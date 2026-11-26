use super::*;

#[test]
fn local_rpm_tracker_rolls_a_bounded_sixty_second_window_and_warms_up() {
    let started = Instant::now();
    let mut tracker = ProviderRpmTracker::new(started);
    assert_eq!(
        tracker.snapshot("provider", started),
        LocalQuotaSnapshot {
            requests_last_60s: 0,
            coverage_seconds: 0,
            status: LocalQuotaTrackingStatus::WarmingUp,
        }
    );
    assert!(tracker.record("provider", started));
    assert_eq!(tracker.snapshot("provider", started).requests_last_60s, 1);
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(59))
            .status,
        LocalQuotaTrackingStatus::WarmingUp
    );
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(60))
            .status,
        LocalQuotaTrackingStatus::Ready
    );
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(60))
            .requests_last_60s,
        0
    );
}

#[test]
fn local_rpm_tracker_reports_capacity_without_fabricating_zero_data() {
    let started = Instant::now();
    let mut tracker = ProviderRpmTracker::new(started);
    for index in 0..MAX_LOCAL_PROVIDER_RPM_TRACKERS {
        assert!(tracker.record(&format!("provider-{index}"), started));
    }
    assert!(!tracker.record("overflow-provider", started));
    let snapshot = tracker.snapshot("overflow-provider", started);
    assert_eq!(snapshot.requests_last_60s, 0);
    assert_eq!(snapshot.status, LocalQuotaTrackingStatus::CapacityLimited);
}
