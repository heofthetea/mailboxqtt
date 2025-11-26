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

struct MessageQueue {
    subscriptions: HashMap<String, Vec<ClientHandle>>,
    /// This is the actual Queue of this Message Queue
    /// (Rust's thread-communication channels are based on queues)
    receiver: Receiver<MQMessage>,
}

impl MessageQueue {
    pub fn new(receiver: Receiver<MQMessage>) -> MessageQueue {
        MessageQueue {
            subscriptions: HashMap::new(),
            receiver,
        }
    }

    /// Primary routine for the message queue.
    /// Awaits messages sent in the unbounded channel and handles them appropriately
    async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                MQMessage::Publish { topic, msg, sender } => self.handle_publish(topic, msg, sender),
                MQMessage::Subscribe { client, topic } => self.subscriptions.entry(topic).or_default().push(client),
            }
        }
    }
    fn handle_publish(&self, topic: String, msg: PublishPacket, sender: ClientHandle) {
        if let Some(subscribers) = self.subscriptions.get(&topic) {
            for subscriber in subscribers {
                _ = subscriber.publish(msg.clone(), sender.clone());
            }
        }
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
