//! Temporary test to check if pedal_dsp::EnvelopeFollower implements Copy.
use pedal_dsp::EnvelopeFollower;

fn assert_copy<T: Copy>() {}
fn assert_clone<T: Clone>() {}

#[test]
fn envelope_follower_is_copy() {
    assert_copy::<EnvelopeFollower>();
}

#[test]
fn envelope_follower_is_clone() {
    assert_clone::<EnvelopeFollower>();
}

#[test]
fn envelope_follower_has_new_and_process() {
    let mut e = EnvelopeFollower::new(10.0, 600.0, 48_000.0);
    let v = e.process(0.5);
    let _ = v;
}
