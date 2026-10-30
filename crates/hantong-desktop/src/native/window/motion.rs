use super::*;
use windows::{UI::ViewManagement::UISettings, core::BOOL};

const DURATION: Duration = Duration::from_millis(140);

struct Transition {
    control: i32,
    from: Option<usize>,
    to: usize,
    started: Instant,
}

#[derive(Default)]
pub(super) struct SelectionMotion(Option<Transition>);
impl SelectionMotion {
    pub(super) fn start(
        &mut self,
        control: i32,
        from: Option<usize>,
        to: Option<usize>,
        now: Instant,
        enabled: bool,
    ) -> bool {
        self.0 = match to {
            Some(to) if enabled && from != Some(to) => Some(Transition {
                control,
                from,
                to,
                started: now,
            }),
            _ => None,
        };
        self.0.is_some()
    }
    pub(super) fn opacity(&self, control: i32, index: usize, selected: bool, now: Instant) -> u8 {
        let Some(t) = &self.0 else {
            return if selected { 255 } else { 0 };
        };
        if t.control != control {
            return if selected { 255 } else { 0 };
        }
        let elapsed = now
            .saturating_duration_since(t.started)
            .as_millis()
            .min(DURATION.as_millis());
        let remaining = 255 - (elapsed * 255 / DURATION.as_millis()) as u32;
        let fade = (remaining * remaining / 255) as u8;
        if index == t.to {
            255 - fade
        } else if Some(index) == t.from {
            fade
        } else {
            0
        }
    }
    pub(super) fn tick(&mut self, now: Instant) -> Option<(i32, bool)> {
        let t = self.0.as_ref()?;
        let control = t.control;
        let active = now.saturating_duration_since(t.started) < DURATION;
        if !active {
            self.0 = None;
        }
        Some((control, active))
    }
    pub(super) fn cancel(&mut self) -> Option<i32> {
        self.0.take().map(|t| t.control)
    }
}

pub(super) fn enabled(high_contrast: bool) -> bool {
    if high_contrast {
        return false;
    }
    let mut client = BOOL(0);
    let system = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(std::ptr::from_mut(&mut client).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    // Both Windows settings must allow motion. A failed query falls back to the
    // immediate selection; no operating-system preference is changed.
    system
        && client.as_bool()
        && UISettings::new()
            .and_then(|s| s.AnimationsEnabled())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ops08_selection_motion_is_bounded_and_idle_without_selection_or_permission() {
        let now = Instant::now();
        let mut motion = SelectionMotion::default();
        for (from, to, enabled) in [
            (Some(1), Some(1), true),
            (None, None, true),
            (Some(1), Some(2), false),
        ] {
            assert!(!motion.start(NAVIGATION, from, to, now, enabled));
            assert!(motion.tick(now).is_none());
        }
        assert!(motion.start(NAVIGATION, Some(1), Some(2), now, true));
        assert_eq!(motion.opacity(NAVIGATION, 1, false, now), 255);
        assert_eq!(motion.opacity(NAVIGATION, 2, true, now), 0);
        let middle = now + Duration::from_millis(70);
        assert!((1..255).contains(&motion.opacity(NAVIGATION, 2, true, middle)));
        assert_eq!(motion.tick(now + DURATION), Some((NAVIGATION, false)));
        assert_eq!(motion.opacity(NAVIGATION, 1, false, now + DURATION), 0);
        assert_eq!(motion.opacity(NAVIGATION, 2, true, now + DURATION), 255);
        assert!(motion.tick(now + DURATION).is_none());
        assert!(motion.start(MESSAGE_LIST, None, Some(0), now, true));
        assert_eq!(motion.cancel(), Some(MESSAGE_LIST));
        assert!(motion.tick(now).is_none());
    }
}
