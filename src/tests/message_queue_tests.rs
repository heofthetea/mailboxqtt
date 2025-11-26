use crate::client::ClientCommand;
use crate::message_queue::TopicTree;
use tokio::sync::mpsc;

use crate::client::ClientHandle;

/// Create a mock ClientHandle for testing
/// The receiver is dropped, so messages sent to this handle go nowhere
fn create_mock_client(id: &str) -> ClientHandle {
    let (tx, _rx) = mpsc::unbounded_channel::<ClientCommand>();
    ClientHandle {
        client_id: id.to_string(),
        sender: tx,
    }
}

#[test]
fn test_exact_topic_subscribe() {
    let mut tree = TopicTree::new();
    let client1 = create_mock_client("client1");
    let client2 = create_mock_client("client2");

    // Subscribe to exact topics
    tree.subscribe(client1.clone(), &["sensor", "temperature"]);
    tree.subscribe(client2.clone(), &["sensor", "humidity"]);

    // Check that clients are in the right place
    assert_eq!(tree.children.len(), 1);
    assert!(tree.children.contains_key("sensor"));

    let sensor_node = tree.children.get("sensor").unwrap();
    assert_eq!(sensor_node.children.len(), 2);
    assert!(sensor_node.children.contains_key("temperature"));
    assert!(sensor_node.children.contains_key("humidity"));

    // Test get_subscribers
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "client1");

    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "humidity"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "client2");

    // Non-existent topic should return empty
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "pressure"], &mut subscribers);
    assert_eq!(subscribers.len(), 0);
}

#[test]
fn test_multi_level_wildcard_subscribe() {
    let mut tree = TopicTree::new();
    let client = create_mock_client("wildcard_client");

    // Subscribe to sensor/#
    tree.subscribe(client.clone(), &["sensor", "#"]);

    // Check structure
    assert_eq!(tree.children.len(), 1);
    let sensor_node = tree.children.get("sensor").unwrap();
    assert_eq!(sensor_node.subscribers_multi_wildcard.len(), 1);
    assert_eq!(sensor_node.subscribers_multi_wildcard[0].client_id, "wildcard_client");

    // Test get_subscribers - should match any topic under sensor/
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "wildcard_client");

    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "humidity", "living"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "wildcard_client");

    // Should NOT match topics not under sensor/
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 0);
}

#[test]
fn test_single_level_wildcard_subscribe() {
    let mut tree = TopicTree::new();
    let client = create_mock_client("plus_client");

    // Subscribe to sensor/+/temperature
    tree.subscribe(client.clone(), &["sensor", "+", "temperature"]);

    // Check structure
    assert_eq!(tree.children.len(), 1);
    let sensor_node = tree.children.get("sensor").unwrap();
    assert_eq!(sensor_node.subscribers_single_wildcard.len(), 1);
    assert_eq!(sensor_node.subscribers_single_wildcard[0].0.client_id, "plus_client");
    assert_eq!(sensor_node.subscribers_single_wildcard[0].1, vec!["temperature"]);

    // Test get_subscribers - should match sensor/<anything>/temperature
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "bedroom", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "plus_client");

    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "kitchen", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "plus_client");

    // Should NOT match wrong depth
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 0);

    // Should NOT match wrong ending
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "bedroom", "humidity"], &mut subscribers);
    assert_eq!(subscribers.len(), 0);
}

#[test]
fn test_mixed_subscriptions() {
    let mut tree = TopicTree::new();
    let exact_client = create_mock_client("exact");
    let wildcard_client = create_mock_client("wildcard");
    let plus_client = create_mock_client("plus");

    // Various subscription patterns
    tree.subscribe(exact_client.clone(), &["home", "living", "temp"]);
    tree.subscribe(wildcard_client.clone(), &["home", "#"]);
    tree.subscribe(plus_client.clone(), &["home", "+", "temp"]);

    // Check root level
    assert_eq!(tree.children.len(), 1);
    let home_node = tree.children.get("home").unwrap();

    // home/# subscriber should be at home level
    assert_eq!(home_node.subscribers_multi_wildcard.len(), 1);
    assert_eq!(home_node.subscribers_multi_wildcard[0].client_id, "wildcard");

    // home/+/temp subscriber should be at home level
    assert_eq!(home_node.subscribers_single_wildcard.len(), 1);
    assert_eq!(home_node.subscribers_single_wildcard[0].0.client_id, "plus");

    // Test get_subscribers
    // Exact match should get exact client + wildcard (home/#) + plus (home/+/temp)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "living", "temp"], &mut subscribers);
    assert_eq!(subscribers.len(), 3); // exact + wildcard + plus
    let client_ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(client_ids.contains(&"exact"));
    assert!(client_ids.contains(&"wildcard"));
    assert!(client_ids.contains(&"plus"));

    // Test + wildcard with new topic (that didn't exist at subscribe time)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "bedroom", "temp"], &mut subscribers);
    assert_eq!(subscribers.len(), 2); // wildcard + plus (but NOT exact)
    let client_ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(client_ids.contains(&"wildcard"));
    assert!(client_ids.contains(&"plus"));

    // Different topic (not matching +/temp pattern) should only get wildcard
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "bedroom", "humidity"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "wildcard");
}

#[test]
fn test_multiple_clients_same_topic() {
    let mut tree = TopicTree::new();
    let client1 = create_mock_client("client1");
    let client2 = create_mock_client("client2");
    let client3 = create_mock_client("client3");

    // All subscribe to the same topic
    tree.subscribe(client1.clone(), &["l1", "l2"]);
    tree.subscribe(client2.clone(), &["l1", "l2"]);
    tree.subscribe(client3.clone(), &["l1", "l2"]);

    // Navigate to the topic node
    let test_node = tree.children.get("l1").unwrap();
    let topic_node = test_node.children.get("l2").unwrap();

    // All three should be subscribers
    assert_eq!(topic_node.subscribers_exact.len(), 3);

    // Test get_subscribers - all three should be returned
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["l1", "l2"], &mut subscribers);
    assert_eq!(subscribers.len(), 3);
    let client_ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(client_ids.contains(&"client1"));
    assert!(client_ids.contains(&"client2"));
    assert!(client_ids.contains(&"client3"));
}

#[test]
fn test_root_level_wildcard() {
    let mut tree = TopicTree::new();
    let client = create_mock_client("catch_all");

    // Subscribe to # (catches everything)
    tree.subscribe(client.clone(), &["#"]);

    // Should be at root level
    assert_eq!(tree.subscribers_multi_wildcard.len(), 1);
    assert_eq!(tree.subscribers_multi_wildcard[0].client_id, "catch_all");

    // Test get_subscribers - should match any topic
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["sensor", "temperature"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "catch_all");

    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "living", "humidity"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "catch_all");

    let mut subscribers = Vec::new();
    tree.get_subscribers(&["anything"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "catch_all");
}

#[test]
fn test_complex_wildcard_combinations() {
    let mut tree = TopicTree::new();
    let exact = create_mock_client("exact");
    let single = create_mock_client("single");
    let multi = create_mock_client("multi");
    let root_catch_all = create_mock_client("root_catch_all");
    let nested_multi = create_mock_client("nested_multi");

    // Complex subscription patterns
    tree.subscribe(exact.clone(), &["home", "bedroom", "temperature", "sensor1"]);
    tree.subscribe(single.clone(), &["home", "+", "temperature", "sensor1"]);
    tree.subscribe(multi.clone(), &["home", "bedroom", "#"]);
    tree.subscribe(root_catch_all.clone(), &["#"]);
    tree.subscribe(nested_multi.clone(), &["home", "#"]);

    // Test exact match: should match exact, single (+), multi (bedroom/#), nested_multi (home/#), root_catch_all (#)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "bedroom", "temperature", "sensor1"], &mut subscribers);
    assert_eq!(subscribers.len(), 5);
    let ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(ids.contains(&"exact"));
    assert!(ids.contains(&"single"));
    assert!(ids.contains(&"multi"));
    assert!(ids.contains(&"nested_multi"));
    assert!(ids.contains(&"root_catch_all"));

    // Test different room: should match single (+), nested_multi (home/#), root_catch_all (#)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "kitchen", "temperature", "sensor1"], &mut subscribers);
    assert_eq!(subscribers.len(), 3);
    let ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(ids.contains(&"single"));
    assert!(ids.contains(&"nested_multi"));
    assert!(ids.contains(&"root_catch_all"));

    // Test different metric in bedroom: should match multi (bedroom/#), nested_multi (home/#), root_catch_all (#)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["home", "bedroom", "humidity"], &mut subscribers);
    assert_eq!(subscribers.len(), 3);
    let ids: Vec<&str> = subscribers.iter().map(|c| c.client_id.as_str()).collect();
    assert!(ids.contains(&"multi"));
    assert!(ids.contains(&"nested_multi"));
    assert!(ids.contains(&"root_catch_all"));

    // Test completely different topic: should only match root_catch_all (#)
    let mut subscribers = Vec::new();
    tree.get_subscribers(&["office", "desk", "light"], &mut subscribers);
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].client_id, "root_catch_all");
}
