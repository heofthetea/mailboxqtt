use std::collections::HashMap;

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

pub struct TopicTree {
    pub subscribers_exact: Vec<ClientHandle>,
    // (client, remaining_segments after the +)
    pub subscribers_single_wildcard: Vec<(ClientHandle, Vec<String>)>,
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
                // Store the client with the remaining segments after the +
                let remaining: Vec<String> = segments[1..].iter().map(|s| s.to_string()).collect();
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

    pub fn get_subscribers<'a>(&'a self, segments: &[&str], subscribers: &mut Vec<&'a ClientHandle>) {
        // Multi-level wildcards match everything from this point
        subscribers.extend(self.subscribers_multi_wildcard.iter());

        // Check for single-level wildcard matches
        // If we have segments left, check if any + wildcard subscribers match
        if let Some(_segment) = segments.get(0) {
            for (client, remaining) in &self.subscribers_single_wildcard {
                // Check if the remaining segments match
                if segments[1..] == remaining.iter().map(|s| s.as_str()).collect::<Vec<_>>()[..] {
                    subscribers.push(client);
                }
            }
        }

        match segments.get(0) {
            None => subscribers.extend(self.subscribers_exact.iter()),
            Some(segment) => {
                if let Some(child) = self.children.get(*segment) {
                    child.get_subscribers(&segments[1..], subscribers);
                }
            }
        }
    }
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
        self.subscriptions.get_subscribers(&topic_nodes, &mut subscribers);
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
