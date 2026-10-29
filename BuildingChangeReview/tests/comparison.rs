use building_change_review::core::dilate;

#[test]
fn weak_evidence_is_buffered_in_source_pixels() {
    let mut scores = vec![0.; 81];
    scores[4 * 9 + 4] = 0.3;
    assert_eq!(dilate(&scores, 9, 9, 2).iter().filter(|v| **v).count(), 25);
    assert_eq!(dilate(&scores, 9, 9, 0).iter().filter(|v| **v).count(), 1);
    scores[4 * 9 + 4] = 0.2;
    assert!(!dilate(&scores, 9, 9, 2).iter().any(|v| *v));
}

#[test]
fn matching_buffer_clips_at_edges_without_wrapping_rows() {
    let mut scores = vec![0.; 25];
    scores[0] = 0.9;
    let output = dilate(&scores, 5, 5, 1);
    assert_eq!(output.iter().filter(|v| **v).count(), 4);
    assert!(output[0] && output[1] && output[5] && output[6]);
    assert!(!output[4] && !output[20]);
}
