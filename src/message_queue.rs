use std::{collections::{HashMap, VecDeque}};

use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::{client::ClientHandle, protocol::publish::PublishPacket};

pub enum MQMessage {
    Subscribe {
        client: ClientHandle,
        topic: String,
    },
    Publish {
        topic: String,
        msg: PublishPacket,
        sender: ClientHandle,
    },
}

struct MessageQueue {
    subscriptions: TopicTree,
    /// This is the actual Queue of this Message Queue
    /// (Rust's thread-communication channels are based on queues)
    receiver: Receiver<MQMessage>,
}

impl MessageQueue {
    pub fn new(receiver: Receiver<MQMessage>) -> MessageQueue {
        MessageQueue {
            subscriptions: TopicTree::new(),
            receiver,
        }
    }

    /// Primary routine for the message queue.
    /// Awaits messages sent in the unbounded channel and handles them appropriately
    async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                MQMessage::Publish { topic, msg, sender } => self.handle_publish(topic, msg, sender),
                MQMessage::Subscribe { client, topic } => self.handle_subscribe(client, topic),
            }
        }
    }
    fn handle_publish(&self, topic: String, msg: PublishPacket, sender: ClientHandle) {
        for subscriber in self.get_subscribers(&topic) {
            _ = subscriber.publish(msg.clone(), sender.clone());
        }
    }

    fn get_subscribers<'a>(&'a self, topic: &str) -> Vec<ClientHandle> {
        let mut subscribers: Vec<&'a ClientHandle> = Vec::new();
        let topic_nodes: Vec<&str> = topic.split('/').collect();
        self.subscriptions
            .get_subscribers(&topic_nodes, &mut subscribers, Vec::new());
        subscribers.iter().cloned().cloned().collect()
    }

    fn handle_subscribe(&mut self, client: ClientHandle, topic: String) {
        let topic_nodes: Vec<&str> = topic.split('/').collect();
        self.subscriptions.subscribe(client, &topic_nodes);
    }
}

/// Handle for inter-thread communication with the Message Queue
/// Holds an Internal reference the the sender of a channel where the receiver is held by the queue
#[derive(Clone)]
pub struct MessageQueueHandle {
    sender: Sender<MQMessage>,
}

impl MessageQueueHandle {
    /// Create a new queue in its own asynchronous thread.
    pub fn start() -> MessageQueueHandle {
        // idk that should be enough messages let's se if it panics it panics
        let (tx, rx) = mpsc::channel(100_000);
        let queue = MessageQueue::new(rx);

        // thread-pin the message queue
        tokio::spawn(async move {
            queue.run().await;
        });
        MessageQueueHandle { sender: tx }
    }

    pub async fn subscribe(&self, client: ClientHandle, topic: String) {
        _ = self.sender.send(MQMessage::Subscribe { client, topic }).await;
    }

    pub async fn publish(&self, topic: String, msg: PublishPacket, sender: ClientHandle) {
        _ = self.sender.send(MQMessage::Publish { topic, msg, sender }).await;
    }
}

pub struct TopicTree {
    pub subscribers_exact: Vec<ClientHandle>,
    // (client, remaining_segments after the +)
    pub subscribers_single_wildcard: Vec<(ClientHandle, VecDeque<String>)>,
    pub subscribers_multi_wildcard: Vec<ClientHandle>,
    // topic segment -> Child node
    pub children: HashMap<String, TopicTree>,
}

impl TopicTree {
    pub fn new() -> TopicTree {
        TopicTree {
            subscribers_exact: Vec::new(),
            subscribers_single_wildcard: Vec::new(),
            subscribers_multi_wildcard: Vec::new(),
            children: HashMap::new(),
        }
    }

    /// Recursively incorporate `client` into the topic subscription tree
    pub fn subscribe(&mut self, client: ClientHandle, segments: &[&str]) {
        match segments.get(0) {
            Some(&"#") => {
                self.subscribers_multi_wildcard.push(client.clone());
                return;
            }
            Some(&"+") => {
                // Store the client with the remaining segments
                // Include the plus because when searching for subscribers, we need to add it at this level
                // and need to pass the information to the next level that it should match everything
                let remaining: VecDeque<String> = segments.iter().map(|s| s.to_string()).collect();
                self.subscribers_single_wildcard.push((client, remaining));
            }
            Some(topic) => {
                self.children
                    .entry((*topic).to_string())
                    .or_insert(TopicTree::new())
                    .subscribe(client.clone(), &segments[1..]);
            }
            None => self.subscribers_exact.push(client), // no more topic left
        }
    }

    /// Get all subscribers in the subtree of `self` for `topic`.
    /// Can handle topics that are not yet included in the subscriber tree with wildcards.
    /// The subscribers are collected in `subscribers` (we avoid returning because of allocation/iterator extension overhead).
    /// The way single wildcards are handled is a bit whack and the only bottleneck that could theroetically exist
    /// (This has become too complicated for me to bother making a runtime performance analysis of this)
    pub fn get_subscribers<'a>(
        &'a self,
        segments: &[&str],
        subscribers: &mut Vec<&'a ClientHandle>,
        mut potential_single_wildcards: Vec<(&'a ClientHandle, VecDeque<String>)>,
    ) {
        // Multi-level wildcards match everything from this point
        subscribers.extend(self.subscribers_multi_wildcard.iter());

        // Okay I give up trying to force iterators onto a side-effectual loop
        // This is for every potential single wildcard match:
        //      (a) Marks it to be removed if its remaining topic is empty
        //      (b) Adds it to the subscribers if it ends in `#`
        //      (c) Pops the first topic segment from its remaining vector
        let mut match_exact_or_remove: Vec<usize> = Vec::new();
        for (i, (client, remaining)) in potential_single_wildcards.iter_mut().enumerate() {
            if remaining.get(0) == Some(&"#".to_owned()) {
                subscribers.push(client);
            }
            remaining.pop_front();
            if remaining.is_empty() {
                match_exact_or_remove.push(i);
                continue;
            }
        }
        // Add all single-level-wildcards at this level to the potential matches
        for (client, remaining) in self.subscribers_single_wildcard.iter() {
            potential_single_wildcards.push((&client, remaining.clone()))
        }

        match segments.get(0) {
            None => {
                subscribers.extend(self.subscribers_exact.iter());
                // Include potential single wildcard matches in the base case if they're remaining vectors are empty
                subscribers.extend(
                    potential_single_wildcards
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| match_exact_or_remove.contains(i))
                        .map(|(_, (client, _))| client),
                )
            }
            Some(segment) => {
                // Removing the potential wildcard matches that have no continuation
                // Not sure how much rust optimizes here but this may result in horrible performance
                potential_single_wildcards = potential_single_wildcards
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !match_exact_or_remove.contains(i)) // remove elements to remove
                    .map(|(_, item)| item) // move out of the enumerate
                    .cloned()
                    .collect();
                if let Some(child) = self.children.get(*segment) {
                    child.get_subscribers(&segments[1..], subscribers, potential_single_wildcards);
                } else {
                    // There's no tree to recurse -> just check for every potential wildcard whether they match and be done
                    for (client, remaining) in potential_single_wildcards {
                        if TopicTree::wildcard_matches_topic(segments, remaining) {
                            subscribers.push(client);
                        }
                    }
                }
            }
        }
    }

    /// Check whether `wildcard` can match `topic`.
    fn wildcard_matches_topic(topic: &[&str], wildcard: VecDeque<String>) -> bool {
        if topic.len() != wildcard.len() {
            // why the fuck do you not supply a way to access the last element explicitly without popping it diggaaaa
            if wildcard.get(wildcard.len() - 1) != Some(&"#".to_owned()) {
                return false;
            }
        }
        for (subtopic, subwilcard) in topic.iter().zip(wildcard) {
            if subtopic != && subwilcard {
                if subwilcard != "+" {
                    return false;
                }
                // immediately match if me made it here and catch a '#' wildcard
                if subwilcard == "#" {
                    return true;
                }
            }
        }

        true
    }
}
