//! Read-only Accessibility probe for the local Notification Center panel.
//!
//! The probe samples from an ordinary worker, never the CGEventTap callback, and does
//! not read notification contents or window metadata. It publishes only a short-lived
//! tri-state snapshot for local routing and optionally logs transitions for diagnostics.

use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::CFString;
use objc2_foundation::NSString;
use oink_foundation::input::NotificationCenterPanelState as PanelState;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const NOTIFICATION_CENTER_BUNDLE_ID: &str = "com.apple.notificationcenterui";
const AX_ERROR_NO_VALUE: i32 = -25212;
const AX_TIMEOUT_SECONDS: f32 = 0.025;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_INTERVAL: Duration = Duration::from_secs(1);
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(1);

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, timeout: f32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementGetTypeID() -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFRelease(value: *const c_void);
}

struct Owned(*const c_void);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: Owned is constructed only from a non-null Create/Copy result.
        unsafe { CFRelease(self.0) };
    }
}

fn state_as_str(state: PanelState) -> &'static str {
    match state {
        PanelState::Open => "open",
        PanelState::Closed => "closed",
        PanelState::Unknown => "unknown",
    }
}

fn classify_focused_window(query: Result<bool, ()>) -> PanelState {
    match query {
        Ok(true) => PanelState::Open,
        Ok(false) => PanelState::Closed,
        Err(()) => PanelState::Unknown,
    }
}

/// Read the current panel state without prompting for Accessibility permission.
/// All unavailable or ambiguous AX results are conservative `Unknown` values.
fn read() -> PanelState {
    // SAFETY: this is the public, read-only trust preflight. Never use the prompt API here.
    if unsafe { AXIsProcessTrusted() } == 0 {
        return PanelState::Unknown;
    }

    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        NOTIFICATION_CENTER_BUNDLE_ID,
    ));
    let Some(app) = apps.firstObject() else {
        return PanelState::Unknown;
    };

    // SAFETY: a live process identifier is passed to the documented AX constructor.
    let raw_app = unsafe { AXUIElementCreateApplication(app.processIdentifier()) };
    if raw_app.is_null() {
        return PanelState::Unknown;
    }
    let app_element = Owned(raw_app);

    // SAFETY: app_element remains live for both calls; timeout bounds AX messaging.
    if unsafe { AXUIElementSetMessagingTimeout(app_element.0, AX_TIMEOUT_SECONDS) } != 0 {
        return PanelState::Unknown;
    }
    let attribute = CFString::from_str("AXFocusedWindow");
    let mut raw_window = std::ptr::null();
    // SAFETY: app_element and attribute are live; AX writes an owned value to raw_window.
    let error = unsafe {
        AXUIElementCopyAttributeValue(app_element.0, (&*attribute as *const CFString).cast(), &mut raw_window)
    };

    if error == AX_ERROR_NO_VALUE && raw_window.is_null() {
        return classify_focused_window(Ok(false));
    }
    if error != 0 || raw_window.is_null() {
        if !raw_window.is_null() {
            // SAFETY: AX returned a non-null owned reference despite an error.
            unsafe { CFRelease(raw_window) };
        }
        return classify_focused_window(Err(()));
    }

    let window = Owned(raw_window);
    // SAFETY: the Copy result is live and type checked before it is treated as an AX element.
    if unsafe { CFGetTypeID(window.0) } != unsafe { AXUIElementGetTypeID() } {
        return classify_focused_window(Err(()));
    }
    classify_focused_window(Ok(true))
}

#[derive(Default)]
struct TransitionTracker(Option<PanelState>);

impl TransitionTracker {
    fn update(&mut self, state: PanelState) -> bool {
        if self.0 == Some(state) {
            return false;
        }
        self.0 = Some(state);
        true
    }

    fn reset(&mut self) {
        self.0 = None;
    }
}

#[derive(Clone, Copy, Debug)]
struct Snapshot {
    state: PanelState,
    observed_at: Option<Instant>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            state: PanelState::Unknown,
            observed_at: None,
        }
    }
}

impl Snapshot {
    fn fresh_state(self, now: Instant, max_age: Duration) -> PanelState {
        match self.observed_at {
            Some(at) if now.saturating_duration_since(at) <= max_age => self.state,
            _ => PanelState::Unknown,
        }
    }
}

static SAMPLER_STARTED: AtomicBool = AtomicBool::new(false);
static INJECTION_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static INJECTION_SESSION_GENERATION: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT: OnceLock<Mutex<Snapshot>> = OnceLock::new();
static WAKE_LOCK: Mutex<()> = Mutex::new(());
static WAKE_WORKER: Condvar = Condvar::new();

fn snapshot_cell() -> &'static Mutex<Snapshot> {
    SNAPSHOT.get_or_init(|| Mutex::new(Snapshot::default()))
}

fn publish(state: PanelState) {
    *snapshot_cell().lock().unwrap_or_else(|e| e.into_inner()) = Snapshot {
        state,
        observed_at: Some(Instant::now()),
    };
}

fn clear_snapshot() {
    *snapshot_cell().lock().unwrap_or_else(|e| e.into_inner()) = Snapshot::default();
}

/// Return only the cached, fresh state; this accessor never performs an AX query.
pub(super) fn current_state() -> PanelState {
    if !INJECTION_SESSION_ACTIVE.load(Ordering::Acquire) {
        return PanelState::Unknown;
    }
    let state = snapshot_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .fresh_state(Instant::now(), SNAPSHOT_MAX_AGE);
    if INJECTION_SESSION_ACTIVE.load(Ordering::Acquire) {
        state
    } else {
        PanelState::Unknown
    }
}

/// Sampling is useful for behavior only while this Mac accepts remote injection.
pub(super) fn set_injection_session_active(active: bool) {
    if INJECTION_SESSION_ACTIVE.swap(active, Ordering::AcqRel) != active {
        INJECTION_SESSION_GENERATION.fetch_add(1, Ordering::AcqRel);
        clear_snapshot();
        WAKE_WORKER.notify_one();
    }
}

fn wait_until_woken() {
    let guard = WAKE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = WAKE_WORKER
        .wait_timeout(guard, IDLE_INTERVAL)
        .unwrap_or_else(|e| e.into_inner());
}

/// Start one process-lifetime worker. AX is queried only for an active injection session
/// or while diagnostics are recording; diagnostic logging remains independently gated.
pub(super) fn start_sampler() {
    if SAMPLER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("oink-notification-center-ax".into())
        .spawn(|| {
            // AX work is low-rate and must not inherit an interactive input-tap QoS.
            // SAFETY: changes only this worker's QoS class.
            let qos = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_DEFAULT, 0) };
            if qos != 0 {
                tracing::warn!(target: "oink_notification_center_probe", error = qos, "could not set AX probe worker QoS");
            }

            let mut transitions = TransitionTracker::default();
            loop {
                let diagnostics = crate::native_lifecycle::lifecycle_enabled();
                let sampling = INJECTION_SESSION_ACTIVE.load(Ordering::Acquire);
                if !sampling && !diagnostics {
                    transitions.reset();
                    clear_snapshot();
                    wait_until_woken();
                    continue;
                }

                let generation = INJECTION_SESSION_GENERATION.load(Ordering::Acquire);
                let state = read();
                if sampling
                    && INJECTION_SESSION_ACTIVE.load(Ordering::Acquire)
                    && INJECTION_SESSION_GENERATION.load(Ordering::Acquire) == generation
                {
                    publish(state);
                }
                if diagnostics {
                    if transitions.update(state) {
                        tracing::info!(
                            target: "oink_notification_center_probe",
                            state = state_as_str(state),
                            attribute = "AXFocusedWindow",
                            "Notification Center panel AX state"
                        );
                    }
                } else {
                    transitions.reset();
                }
                std::thread::sleep(SAMPLE_INTERVAL);
            }
        });

    if let Err(error) = spawned {
        SAMPLER_STARTED.store(false, Ordering::Release);
        tracing::warn!(target: "oink_notification_center_probe", %error, "could not start AX probe worker");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_window_query_maps_to_tri_state_conservatively() {
        assert_eq!(classify_focused_window(Ok(true)), PanelState::Open);
        assert_eq!(classify_focused_window(Ok(false)), PanelState::Closed);
        assert_eq!(classify_focused_window(Err(())), PanelState::Unknown);
    }

    #[test]
    fn transition_tracker_emits_only_changes_and_resets_between_recordings() {
        let mut tracker = TransitionTracker::default();
        assert!(tracker.update(PanelState::Unknown));
        assert!(!tracker.update(PanelState::Unknown));
        assert!(tracker.update(PanelState::Open));
        assert!(!tracker.update(PanelState::Open));
        assert!(tracker.update(PanelState::Closed));
        tracker.reset();
        assert!(tracker.update(PanelState::Closed));
    }

    #[test]
    fn snapshot_expiry_is_conservative_and_unknown_is_never_closed() {
        let now = Instant::now();
        let fresh_open = Snapshot {
            state: PanelState::Open,
            observed_at: Some(now - Duration::from_millis(250)),
        };
        assert_eq!(fresh_open.fresh_state(now, SNAPSHOT_MAX_AGE), PanelState::Open);

        let stale_open = Snapshot {
            state: PanelState::Open,
            observed_at: Some(now - SNAPSHOT_MAX_AGE - Duration::from_millis(1)),
        };
        assert_eq!(stale_open.fresh_state(now, SNAPSHOT_MAX_AGE), PanelState::Unknown);
        assert_eq!(
            Snapshot::default().fresh_state(now, SNAPSHOT_MAX_AGE),
            PanelState::Unknown
        );

        let fresh_unknown = Snapshot {
            state: PanelState::Unknown,
            observed_at: Some(now),
        };
        assert_eq!(fresh_unknown.fresh_state(now, SNAPSHOT_MAX_AGE), PanelState::Unknown);
    }
}
