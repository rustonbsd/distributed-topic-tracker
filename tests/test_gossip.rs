use distributed_topic_tracker::TopicId;


#[test]
fn test_topic_id_creation() {
    let topic_id = TopicId::new("test-topic".to_string());
    assert_eq!(topic_id.raw(), "test-topic");
    assert_eq!(topic_id.hash().len(), 32);

    // Same input should produce same hash
    let topic_id2 = TopicId::new("test-topic".to_string());
    assert_eq!(topic_id.hash(), topic_id2.hash());

    // Different input should produce different hash
    let topic_id3 = TopicId::new("different-topic".to_string());
    assert_ne!(topic_id.hash(), topic_id3.hash());
}
