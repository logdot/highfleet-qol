pub(super) const INITIAL_HEALTH: f32 = 10.0;

pub(super) fn apply_impact(health: &mut f32, normal_impulse: f32) -> bool {
    if !health.is_finite() || *health <= 0.0 || !normal_impulse.is_finite() || normal_impulse <= 0.0
    {
        return false;
    }
    // Native hull damage is 0.8 * Box2D normal impulse. Instruments are fragile:
    // every resolved round impact does at least one HP, even for tiny fragments.
    *health = (*health - (normal_impulse * 0.8).max(1.0)).max(0.0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_round_impacts_destroy_the_instrument() {
        let mut health = INITIAL_HEALTH;
        for _ in 0..INITIAL_HEALTH.ceil() as usize {
            let previous = health;
            assert!(apply_impact(&mut health, 0.01));
            assert_eq!(health, (previous - 1.0).max(0.0));
        }
        assert_eq!(health, 0.0);
        assert!(!apply_impact(&mut health, 0.01));
    }

    #[test]
    fn invalid_or_unresolved_contacts_leave_health_unchanged() {
        for impulse in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut health = INITIAL_HEALTH;
            assert!(!apply_impact(&mut health, impulse));
            assert_eq!(health, INITIAL_HEALTH);
        }
    }
}
