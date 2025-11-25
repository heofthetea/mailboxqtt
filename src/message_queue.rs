use std::collections::{HashMap};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{client::ClientHandle, protocol::publish_packet::PublishPacket};

pub enum MQMessage {
    Subscribe { client: ClientHandle, topic: String },
    Publish { topic: String, msg: PublishPacket },
}

struct MessageQueue {
    subscriptions: HashMap<String, Vec<ClientHandle>>,
    receiver: UnboundedReceiver<MQMessage>,
}

impl MessageQueue {
    pub fn new(receiver: UnboundedReceiver<MQMessage>) -> MessageQueue {
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
                MQMessage::Publish { topic, msg } => {
                     if let Some(subscribers) = self.subscriptions.get(&topic) {
                        for subscriber in subscribers {
                            _ = subscriber.publish(msg.clone());
                        }
                    }
                }
                MQMessage::Subscribe { client, topic } => {
                    self.subscriptions.entry(topic).or_default().push(client);
                }
            }
        }
    }
}

/// Handle for inter-thread communication with the Message Queue
/// Holds an Internal reference the the sender of a channel where the receiver is held by the queue
#[derive(Clone)]
pub struct MessageQueueHandle {
    sender: UnboundedSender<MQMessage>,
}

impl MessageQueueHandle {
    /// Create a new queue in its own asynchronous thread.
    pub fn start() -> MessageQueueHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        let actor = MessageQueue::new(rx);

        tokio::spawn(async move {
            actor.run().await;
        });
        MessageQueueHandle { sender: tx }
    }

    pub fn subscribe(&self, client: ClientHandle, topic: String) {
        _ = self.sender.send(MQMessage::Subscribe { client, topic })
    }

    pub fn publish(&self, topic: String, msg: PublishPacket) {
        _ = self.sender.send(MQMessage::Publish { topic, msg });
    }
}
