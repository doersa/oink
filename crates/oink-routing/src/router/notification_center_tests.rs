use super::*;

const ME: DeviceId = DeviceId([1; 32]);
const PEER: DeviceId = DeviceId([2; 32]);

fn router(os: Os) -> Router {
    Router::new(
        RouterConfig::new(
            oink_foundation::session::Boot {
                device_id: ME,
                nonce: [1; 16],
            },
            "test",
            os,
        ),
        Arc::new(crate::SimpleLayout::default()),
    )
}

fn gesture(phase: GesturePhase) -> Gesture {
    Gesture::NotificationCenter { phase, native: None }
}

fn native_gesture(phase: GesturePhase) -> Gesture {
    Gesture::NotificationCenter {
        phase,
        native: Some(oink_foundation::input::NotificationCenterNativeFields {
            motion: 9,
            flavor: 1,
            swipe_phase: match phase {
                GesturePhase::Began => 1,
                GesturePhase::Changed => 2,
                GesturePhase::Ended => 4,
                GesturePhase::Cancelled => 8,
            },
            progress: 0.25,
            progress_f32_bits: i64::from(0.25_f32.to_bits()),
            gesture_progress: 0.5,
            inverted_from_device: 0,
            raw_field_129: 0.0,
            raw_field_130: 0.0,
        }),
    }
}

fn message(phase: GesturePhase) -> Message {
    Message::Input(InputMsg::Gesture {
        source: SourceKind::Touchpad,
        gesture: gesture(phase),
    })
}

fn scroll_message(dx: f64, dy: f64, phase: ScrollPhase) -> Message {
    Message::Input(InputMsg::Scroll {
        source: SourceKind::Touchpad,
        dx,
        dy,
        unit: ScrollAmount::Pixels,
        phase,
        wheel_steps: None,
    })
}

fn ready_for_remote_input(r: &mut Router) {
    r.focus_joined = true;
    r.fresh_join = false;
    r.epoch = 1;
    r.focus_scope = Some(r.new_focus_scope());
    let peer = r.peers.get_mut(&PEER).unwrap();
    peer.focus_ready = true;
    peer.feel.natural_scroll_touchpad = true;
}

fn notification_center_injections(r: &mut Router) -> Vec<InjectEvent> {
    r.drain_outputs()
        .into_iter()
        .filter_map(|output| match output {
            Output::Inject(event @ InjectEvent::NotificationCenter) => Some(event),
            _ => None,
        })
        .collect()
}

fn native_close_gestures(outputs: &[Output]) -> Vec<Gesture> {
    outputs
        .iter()
        .filter_map(|output| match output {
            Output::Inject(InjectEvent::Gesture(
                gesture @ Gesture::NotificationCenter {
                    native: Some(fields), ..
                },
            )) if fields.motion == 1 => Some(*gesture),
            _ => None,
        })
        .collect()
}

fn native_gesture_injections(r: &mut Router) -> Vec<Gesture> {
    r.drain_outputs()
        .into_iter()
        .filter_map(|output| match output {
            Output::Inject(InjectEvent::Gesture(gesture @ Gesture::NotificationCenter { native: Some(_), .. })) => {
                Some(gesture)
            }
            _ => None,
        })
        .collect()
}

fn native_message(phase: GesturePhase) -> Message {
    Message::Input(InputMsg::Gesture {
        source: SourceKind::Touchpad,
        gesture: native_gesture(phase),
    })
}

#[test]
fn source_locks_notification_center_to_the_mac_focused_at_gesture_start() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    r.set_peer_os(PEER, Os::MacOS, 0);
    r.set_peer_notification_center_actions(PEER, true);
    r.holder = PEER;
    r.claim = PEER;
    r.out.clear();

    assert_eq!(
        r.local_gesture(SourceKind::Touchpad, gesture(GesturePhase::Began)),
        Disposition::Swallow
    );
    assert_eq!(
        r.local_gesture(SourceKind::Touchpad, gesture(GesturePhase::Changed)),
        Disposition::Swallow
    );
    // Focus moves during the gesture; the terminal phase must remain on its original target.
    r.holder = ME;
    assert_eq!(
        r.local_gesture(SourceKind::Touchpad, gesture(GesturePhase::Ended)),
        Disposition::Swallow
    );

    let sent: Vec<_> = r
        .out
        .iter()
        .filter_map(|output| match output {
            Output::Send {
                to,
                msg: Message::Input(InputMsg::Gesture { gesture, .. }),
                ..
            } => Some((*to, *gesture)),
            _ => None,
        })
        .collect();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|(to, _)| *to == PEER));
    assert_eq!(sent[0].1, gesture(GesturePhase::Began));
    assert_eq!(sent[2].1, gesture(GesturePhase::Ended));
}

#[test]
fn source_forwards_native_metadata_live_to_the_locked_mac_peer() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    r.set_peer_os(PEER, Os::MacOS, 0);
    r.set_peer_notification_center_actions(PEER, true);
    r.holder = PEER;
    r.claim = PEER;
    r.out.clear();

    for phase in [GesturePhase::Began, GesturePhase::Changed, GesturePhase::Ended] {
        r.local_gesture(SourceKind::Touchpad, native_gesture(phase));
    }
    let sent: Vec<_> = r
        .out
        .iter()
        .filter_map(|output| match output {
            Output::Send {
                to: PEER,
                msg: Message::Input(InputMsg::Gesture { gesture, .. }),
                ..
            } => Some(*gesture),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent,
        [
            native_gesture(GesturePhase::Began),
            native_gesture(GesturePhase::Changed),
            native_gesture(GesturePhase::Ended),
        ]
    );
}

#[test]
fn source_drops_action_when_forwarding_or_peer_capability_is_missing() {
    for (forward, capable) in [(false, true), (true, false)] {
        let mut r = router(Os::MacOS);
        r.test_connect(PEER, 0);
        r.set_peer_os(PEER, Os::MacOS, 0);
        r.set_peer_notification_center_actions(PEER, capable);
        r.holder = PEER;
        r.claim = PEER;
        r.cfg.settings.forward_gestures = forward;
        r.out.clear();

        for phase in [GesturePhase::Began, GesturePhase::Ended] {
            assert_eq!(
                r.local_gesture(SourceKind::Touchpad, gesture(phase)),
                Disposition::Swallow
            );
        }
        assert!(!r.out.iter().any(|output| matches!(
            output,
            Output::Send {
                msg: Message::Input(InputMsg::Gesture {
                    gesture: Gesture::NotificationCenter { .. },
                    ..
                }),
                ..
            }
        )));
    }
}

#[test]
fn destination_executes_one_action_only_after_a_complete_ended_sequence() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.out.clear();
    r.test_message(PEER, message(GesturePhase::Began), 1);
    r.test_message(PEER, message(GesturePhase::Changed), 2);
    r.test_message(PEER, message(GesturePhase::Ended), 3);
    assert_eq!(
        notification_center_injections(&mut r),
        [InjectEvent::NotificationCenter]
    );

    r.test_message(PEER, message(GesturePhase::Ended), 4);
    assert!(
        notification_center_injections(&mut r).is_empty(),
        "duplicate end must not toggle twice"
    );

    r.test_message(PEER, message(GesturePhase::Began), 5);
    r.test_message(PEER, message(GesturePhase::Cancelled), 6);
    assert!(
        notification_center_injections(&mut r).is_empty(),
        "cancelled gesture must not toggle"
    );
}

#[test]
fn open_target_enters_close_phase_on_small_horizontal_motion_without_a_buffered_burst() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();

    let mut outputs = Vec::new();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    let begin_outputs = r.drain_outputs();
    assert!(
        native_close_gestures(&begin_outputs).is_empty(),
        "do not start a native gesture for a zero-motion scroll begin"
    );
    outputs.extend(begin_outputs);

    r.test_message(PEER, scroll_message(16.0, 0.0, ScrollPhase::Changed), 2);
    let first_motion = r.drain_outputs();
    let first_native = native_close_gestures(&first_motion);
    assert_eq!(
        first_native.iter().map(Gesture::phase).collect::<Vec<_>>(),
        [Some(GesturePhase::Began), Some(GesturePhase::Changed)],
        "clear horizontal intent enters the close phase without waiting for a distance threshold"
    );
    assert!(matches!(
        first_native.last(),
        Some(Gesture::NotificationCenter { native: Some(fields), .. })
            if fields.progress == 1.0 / 3.0
    ));
    outputs.extend(first_motion);

    r.test_message(PEER, scroll_message(16.0, 0.0, ScrollPhase::Changed), 3);
    let continued = r.drain_outputs();
    let continued_native = native_close_gestures(&continued);
    assert!(matches!(
        continued_native.as_slice(),
        [Gesture::NotificationCenter { phase: GesturePhase::Changed, native: Some(fields) }]
            if fields.progress == 2.0 / 3.0
    ));
    outputs.extend(continued);

    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 4);
    let ended = r.drain_outputs();
    let ended_native = native_close_gestures(&ended);
    assert!(
        matches!(
            ended_native.as_slice(),
            [Gesture::NotificationCenter { phase: GesturePhase::Ended, native: Some(fields) }]
                if fields.progress == 2.0 / 3.0
        ),
        "release settles from the measured progress, not a forced 1.0: {ended_native:?}"
    );
    outputs.extend(ended);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::MomentumBegan), 5);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Momentum), 6);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::MomentumEnded), 7);
    outputs.extend(r.drain_outputs());

    let native = native_close_gestures(&outputs);
    assert_eq!(
        native.iter().map(Gesture::phase).collect::<Vec<_>>(),
        [
            Some(GesturePhase::Began),
            Some(GesturePhase::Changed),
            Some(GesturePhase::Changed),
            Some(GesturePhase::Ended)
        ]
    );
    assert!(
        outputs.iter().all(|output| !matches!(
            output,
            Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
        )),
        "converted input must not also scroll or toggle"
    );
    assert!(native.iter().all(|gesture| matches!(
        gesture,
        Gesture::NotificationCenter { native: Some(fields), .. }
            if fields.motion == 1 && fields.flavor == 1 && fields.is_valid_for_phase(gesture.phase().unwrap())
    )));
}

#[test]
fn subpixel_closeward_swipe_enters_native_close_phase_without_ordinary_scroll() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    r.test_message(PEER, scroll_message(0.25, 0.0, ScrollPhase::Changed), 2);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 3);
    let outputs = r.drain_outputs();
    let native = native_close_gestures(&outputs);
    assert_eq!(
        native.iter().map(Gesture::phase).collect::<Vec<_>>(),
        [
            Some(GesturePhase::Began),
            Some(GesturePhase::Changed),
            Some(GesturePhase::Ended)
        ],
        "subpixel horizontal intent must enter the native close lifecycle"
    );
    assert!(matches!(
        native.get(1),
        Some(Gesture::NotificationCenter { native: Some(fields), .. })
            if fields.progress == 0.25 / 48.0
    ));
    assert!(outputs.iter().all(|output| !matches!(
        output,
        Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
    )));
}

#[test]
fn close_direction_respects_the_source_touchpad_natural_scroll_setting() {
    for (natural, dx) in [(true, 32.0), (false, -32.0)] {
        let mut r = router(Os::MacOS);
        r.test_connect(PEER, 0);
        ready_for_remote_input(&mut r);
        r.peers.get_mut(&PEER).unwrap().feel.natural_scroll_touchpad = natural;
        r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
        r.out.clear();
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
        r.test_message(PEER, scroll_message(dx, 0.0, ScrollPhase::Changed), 2);
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 3);
        let outputs = r.drain_outputs();
        let native = native_close_gestures(&outputs);
        assert_eq!(
            native.iter().map(Gesture::phase).collect::<Vec<_>>(),
            [
                Some(GesturePhase::Began),
                Some(GesturePhase::Changed),
                Some(GesturePhase::Ended)
            ],
            "natural_scroll_touchpad={natural}, dx={dx}"
        );
        assert!(matches!(
            native.last(),
            Some(Gesture::NotificationCenter { native: Some(fields), .. })
                if fields.progress == 2.0 / 3.0
        ));
        assert!(outputs.iter().all(|output| !matches!(
            output,
            Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
        )));
    }
}

#[test]
fn active_close_progress_reverses_and_resumes_for_both_scroll_preferences() {
    for natural in [true, false] {
        let mut r = router(Os::MacOS);
        r.test_connect(PEER, 0);
        ready_for_remote_input(&mut r);
        r.peers.get_mut(&PEER).unwrap().feel.natural_scroll_touchpad = natural;
        r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
        r.out.clear();
        let close_sign = if natural { 1.0 } else { -1.0 };
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
        for (index, dx) in [24.0, -12.0, -24.0, 24.0].into_iter().enumerate() {
            r.test_message(
                PEER,
                scroll_message(close_sign * dx, 0.0, ScrollPhase::Changed),
                2 + index as u64,
            );
        }
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 6);
        let outputs = r.drain_outputs();
        let native = native_close_gestures(&outputs);
        assert_eq!(
            native.iter().map(Gesture::phase).collect::<Vec<_>>(),
            [
                Some(GesturePhase::Began),
                Some(GesturePhase::Changed),
                Some(GesturePhase::Changed),
                Some(GesturePhase::Changed),
                Some(GesturePhase::Changed),
                Some(GesturePhase::Ended)
            ]
        );
        let progress: Vec<_> = native
            .iter()
            .map(|gesture| match gesture {
                Gesture::NotificationCenter {
                    native: Some(fields), ..
                } => {
                    assert_eq!(fields.progress_f32_bits, i64::from((fields.progress as f32).to_bits()));
                    fields.progress
                }
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(progress, [0.0, 0.5, 0.25, 0.0, 0.5, 0.5]);
        assert!(outputs.iter().all(|output| !matches!(
            output,
            Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
        )));
    }
}

#[test]
fn releasing_at_the_fully_open_endpoint_does_not_complete_the_close() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    r.test_message(PEER, scroll_message(24.0, 0.0, ScrollPhase::Changed), 2);
    r.test_message(PEER, scroll_message(-24.0, 0.0, ScrollPhase::Changed), 3);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 4);
    let outputs = r.drain_outputs();
    let native = native_close_gestures(&outputs);
    assert!(matches!(
        native.last(),
        Some(Gesture::NotificationCenter { phase: GesturePhase::Ended, native: Some(fields) })
            if fields.progress == 0.0
    ));
    assert!(outputs.iter().all(|output| !matches!(
        output,
        Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
    )));
}

#[test]
fn closed_unknown_and_wrong_direction_keep_ordinary_scroll_delivery() {
    for (state, dx, dy) in [
        (oink_foundation::input::NotificationCenterPanelState::Closed, -32.0, 0.0),
        (
            oink_foundation::input::NotificationCenterPanelState::Unknown,
            -32.0,
            0.0,
        ),
        (oink_foundation::input::NotificationCenterPanelState::Open, -32.0, 0.0),
        (oink_foundation::input::NotificationCenterPanelState::Open, -32.0, -28.0),
        (oink_foundation::input::NotificationCenterPanelState::Open, 0.0, 40.0),
    ] {
        let mut r = router(Os::MacOS);
        r.test_connect(PEER, 0);
        ready_for_remote_input(&mut r);
        r.set_local_notification_center_state(state);
        r.out.clear();
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
        r.test_message(PEER, scroll_message(dx, dy, ScrollPhase::Changed), 2);
        r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 3);
        let outputs = r.drain_outputs();
        assert_eq!(
            outputs
                .iter()
                .filter(|output| matches!(output, Output::Inject(InjectEvent::Scroll { .. })))
                .count(),
            3,
            "state={state:?}, delta=({dx}, {dy})"
        );
        assert!(outputs.iter().all(|output| !matches!(
            output,
            Output::Inject(InjectEvent::NotificationCenter)
                | Output::Inject(InjectEvent::Gesture(Gesture::NotificationCenter {
                    phase: GesturePhase::Ended,
                    native: Some(_),
                }))
        )));
        let native = native_close_gestures(&outputs);
        assert!(
            native.is_empty(),
            "closed/unknown panels and non-close directions never start a native close"
        );
    }
}

#[test]
fn ambiguous_scroll_prefix_remains_bounded_and_flushes_once_on_end() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    r.test_message(PEER, scroll_message(4.0, 4.0, ScrollPhase::Changed), 2);
    let pending = r.drain_outputs();
    assert!(
        native_close_gestures(&pending).is_empty()
            && pending
                .iter()
                .all(|output| !matches!(output, Output::Inject(InjectEvent::Scroll { .. }))),
        "ambiguous diagonal movement remains buffered until direction or termination is known"
    );

    r.tick(300_000);
    let after_tick = r.drain_outputs();
    assert!(
        native_close_gestures(&after_tick).is_empty()
            && after_tick
                .iter()
                .all(|output| !matches!(output, Output::Inject(InjectEvent::Scroll { .. }))),
        "elapsed time alone must not discard a potentially slow closeward gesture"
    );

    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 300_001);
    let outputs = r.drain_outputs();
    assert_eq!(
        outputs
            .iter()
            .filter(|output| matches!(output, Output::Inject(InjectEvent::Scroll { .. })))
            .count(),
        3,
        "an ambiguous gesture must fall back to ordinary scrolling exactly once"
    );
    assert!(native_close_gestures(&outputs).is_empty());
}

#[test]
fn slow_closeward_intent_after_legacy_pending_limits_still_enters_native_phase() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    for i in 0..13 {
        r.test_message(
            PEER,
            scroll_message(1.0, 1.0, ScrollPhase::Changed),
            30_000 + i * 30_000,
        );
    }
    r.test_message(PEER, scroll_message(14.25, 0.0, ScrollPhase::Changed), 420_000);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 450_000);
    let outputs = r.drain_outputs();
    let native = native_close_gestures(&outputs);
    assert_eq!(
        native.iter().map(Gesture::phase).collect::<Vec<_>>(),
        [
            Some(GesturePhase::Began),
            Some(GesturePhase::Changed),
            Some(GesturePhase::Ended)
        ],
        "a slow, small horizontal intent should classify even after 12 frames and 250 ms"
    );
    assert!(matches!(
        native.get(1),
        Some(Gesture::NotificationCenter { native: Some(fields), .. })
            if fields.progress == 27.25 / 48.0
    ));
    assert!(outputs.iter().all(|output| !matches!(
        output,
        Output::Inject(InjectEvent::Scroll { .. } | InjectEvent::NotificationCenter)
    )));
}

#[test]
fn pending_close_candidate_falls_back_if_notification_center_stops_being_open() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    r.test_message(PEER, scroll_message(4.0, 4.0, ScrollPhase::Changed), 2);
    let pending = r.drain_outputs();
    assert!(native_close_gestures(&pending).is_empty());
    assert!(
        pending
            .iter()
            .all(|output| !matches!(output, Output::Inject(InjectEvent::Scroll { .. })))
    );

    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Closed);
    r.test_message(PEER, scroll_message(14.0, 0.0, ScrollPhase::Changed), 3);
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Ended), 4);
    let outputs = r.drain_outputs();
    assert_eq!(
        outputs
            .iter()
            .filter(|output| matches!(output, Output::Inject(InjectEvent::Scroll { .. })))
            .count(),
        4,
        "state change cancels candidacy but preserves each scroll frame once"
    );
    assert!(native_close_gestures(&outputs).is_empty());
}

#[test]
fn pause_cancels_an_active_synthetic_close_instead_of_finishing_or_toggling() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.set_local_notification_center_state(oink_foundation::input::NotificationCenterPanelState::Open);
    r.out.clear();
    r.test_message(PEER, scroll_message(0.0, 0.0, ScrollPhase::Began), 1);
    r.test_message(PEER, scroll_message(32.0, 0.0, ScrollPhase::Changed), 2);
    r.drain_outputs();

    r.set_paused(true);
    let outputs = r.drain_outputs();
    assert!(outputs.iter().any(|output| matches!(
        output,
        Output::Inject(InjectEvent::Gesture(Gesture::NotificationCenter {
            phase: GesturePhase::Cancelled,
            native: Some(fields),
        })) if fields.motion == 1 && fields.swipe_phase == 8
    )));
    assert!(outputs.iter().all(|output| !matches!(
        output,
        Output::Inject(InjectEvent::NotificationCenter)
            | Output::Inject(InjectEvent::Gesture(Gesture::NotificationCenter {
                phase: GesturePhase::Ended,
                ..
            }))
    )));
}

#[test]
fn destination_streams_native_frames_and_requires_a_complete_sequence() {
    let mut r = router(Os::MacOS);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.out.clear();

    // Orphaned phases and duplicate terminal frames are never injected.
    r.test_message(PEER, native_message(GesturePhase::Changed), 1);
    r.test_message(PEER, native_message(GesturePhase::Ended), 2);
    assert!(native_gesture_injections(&mut r).is_empty());
    assert!(notification_center_injections(&mut r).is_empty());

    r.test_message(PEER, native_message(GesturePhase::Began), 3);
    r.test_message(PEER, native_message(GesturePhase::Changed), 4);
    r.test_message(PEER, native_message(GesturePhase::Ended), 5);
    let delivered = native_gesture_injections(&mut r);
    assert_eq!(
        delivered,
        [
            native_gesture(GesturePhase::Began),
            native_gesture(GesturePhase::Changed),
            native_gesture(GesturePhase::Ended),
        ]
    );
    assert!(notification_center_injections(&mut r).is_empty());

    r.test_message(PEER, native_message(GesturePhase::Ended), 6);
    assert!(native_gesture_injections(&mut r).is_empty());

    r.test_message(PEER, native_message(GesturePhase::Began), 7);
    r.test_message(PEER, native_message(GesturePhase::Cancelled), 8);
    assert_eq!(
        native_gesture_injections(&mut r),
        [
            native_gesture(GesturePhase::Began),
            native_gesture(GesturePhase::Cancelled),
        ]
    );
    r.test_message(PEER, native_message(GesturePhase::Changed), 9);
    assert!(native_gesture_injections(&mut r).is_empty());
    assert!(notification_center_injections(&mut r).is_empty());

    // A duplicate begin closes the previous native lifecycle before restarting.
    r.test_message(PEER, native_message(GesturePhase::Began), 10);
    r.test_message(PEER, native_message(GesturePhase::Began), 11);
    assert_eq!(
        native_gesture_injections(&mut r),
        [
            native_gesture(GesturePhase::Began),
            native_gesture(GesturePhase::Cancelled),
            native_gesture(GesturePhase::Began),
        ]
    );

    // A phase-only terminal arriving mid-native-stream cancels instead of
    // replaying an incomplete native event or falling back to a second toggle.
    r.test_message(PEER, message(GesturePhase::Ended), 12);
    assert_eq!(
        native_gesture_injections(&mut r),
        [native_gesture(GesturePhase::Cancelled)]
    );
    assert!(notification_center_injections(&mut r).is_empty());

    // If the initial phase lacks candidate metadata, stay on the safe shortcut
    // fallback for the complete gesture rather than switching replay midstream.
    r.test_message(PEER, message(GesturePhase::Began), 13);
    r.test_message(PEER, native_message(GesturePhase::Changed), 14);
    r.test_message(PEER, native_message(GesturePhase::Ended), 15);
    assert_eq!(
        notification_center_injections(&mut r),
        [InjectEvent::NotificationCenter]
    );
    assert!(native_gesture_injections(&mut r).is_empty());
}

#[test]
fn non_mac_destination_never_maps_notification_center_to_a_generic_gesture() {
    let mut r = router(Os::Windows);
    r.test_connect(PEER, 0);
    ready_for_remote_input(&mut r);
    r.out.clear();
    r.test_message(PEER, message(GesturePhase::Began), 1);
    r.test_message(PEER, message(GesturePhase::Ended), 2);
    assert!(!r.out.iter().any(|output| matches!(
        output,
        Output::Inject(InjectEvent::NotificationCenter | InjectEvent::Gesture(_))
    )));
    {
        r.out.clear();
        r.test_message(PEER, native_message(GesturePhase::Began), 3);
        r.test_message(PEER, native_message(GesturePhase::Ended), 4);
        assert!(!r.out.iter().any(|output| matches!(
            output,
            Output::Inject(InjectEvent::NotificationCenter | InjectEvent::Gesture(_))
        )));
    }
}
