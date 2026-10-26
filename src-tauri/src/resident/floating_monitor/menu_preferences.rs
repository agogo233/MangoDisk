//! Native menu choices and deterministic changes to floating preferences.
use crate::resident::preference_schema::{
    FloatingBackground, FloatingFontSize, FloatingLayout, FloatingPreferences,
};

pub(super) const FONT_SIZES: [(&str, FloatingFontSize); 4] = [
    ("standard", FloatingFontSize::Standard),
    ("larger", FloatingFontSize::Larger),
    ("large", FloatingFontSize::Large),
    ("extraLarge", FloatingFontSize::ExtraLarge),
];
pub(super) const BACKGROUNDS: [(&str, FloatingBackground); 7] = [
    ("default", FloatingBackground::Default),
    ("darkGray", FloatingBackground::DarkGray),
    ("inkBlue", FloatingBackground::InkBlue),
    ("warmWhite", FloatingBackground::WarmWhite),
    ("lightGray", FloatingBackground::LightGray),
    ("black", FloatingBackground::Black),
    ("white", FloatingBackground::White),
];
pub(super) const OPACITIES: [u8; 6] = [100, 90, 80, 70, 60, 50];

pub(super) fn apply(preferences: &mut FloatingPreferences, action: &str) -> bool {
    match action {
        "floating-horizontal" => preferences.layout = FloatingLayout::Horizontal,
        "floating-vertical" => preferences.layout = FloatingLayout::Vertical,
        "floating-dividers" => preferences.dividers = !preferences.dividers,
        "floating-bold" => preferences.bold = !preferences.bold,
        "floating-top" => preferences.always_on_top = !preferences.always_on_top,
        "floating-lock" => preferences.locked = !preferences.locked,
        "floating-snap" => preferences.snap_to_edges = !preferences.snap_to_edges,
        "floating-fullscreen" => preferences.hide_on_fullscreen = !preferences.hide_on_fullscreen,
        _ => {
            if let Some((_, value)) = action
                .strip_prefix("floating-font-")
                .and_then(|id| FONT_SIZES.iter().find(|(key, _)| *key == id))
            {
                preferences.font_size = *value;
            } else if let Some((_, value)) = action
                .strip_prefix("floating-background-")
                .and_then(|id| BACKGROUNDS.iter().find(|(key, _)| *key == id))
            {
                preferences.background = *value;
            } else if let Some(value) = action
                .strip_prefix("floating-opacity-")
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| OPACITIES.contains(value))
            {
                preferences.background_opacity = value;
            } else {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_values_are_idempotent_and_preserve_other_fields() {
        let original = FloatingPreferences {
            locked: true,
            click_through: true,
            bold: true,
            ..FloatingPreferences::default()
        };
        for (id, value) in FONT_SIZES {
            let mut preferences = original.clone();
            let mut expected = original.clone();
            expected.font_size = value;
            for _ in 0..2 {
                assert!(apply(&mut preferences, &format!("floating-font-{id}")));
                assert_eq!(preferences, expected);
            }
        }
        for (id, value) in BACKGROUNDS {
            let mut preferences = original.clone();
            let mut expected = original.clone();
            expected.background = value;
            for _ in 0..2 {
                assert!(apply(
                    &mut preferences,
                    &format!("floating-background-{id}")
                ));
                assert_eq!(preferences, expected);
            }
        }
        for value in OPACITIES {
            let mut preferences = original.clone();
            let mut expected = original.clone();
            expected.background_opacity = value;
            for _ in 0..2 {
                assert!(apply(
                    &mut preferences,
                    &format!("floating-opacity-{value}")
                ));
                assert_eq!(preferences, expected);
            }
        }
    }

    #[test]
    fn toggles_change_only_the_requested_field_and_can_be_reversed() {
        let original = FloatingPreferences::default();
        for action in [
            "floating-dividers",
            "floating-bold",
            "floating-top",
            "floating-lock",
            "floating-snap",
            "floating-fullscreen",
        ] {
            let mut preferences = original.clone();
            let mut expected = original.clone();
            match action {
                "floating-dividers" => expected.dividers = false,
                "floating-bold" => expected.bold = true,
                "floating-top" => expected.always_on_top = false,
                "floating-lock" => expected.locked = true,
                "floating-snap" => expected.snap_to_edges = false,
                "floating-fullscreen" => expected.hide_on_fullscreen = false,
                _ => unreachable!(),
            }
            assert!(apply(&mut preferences, action));
            assert_eq!(preferences, expected);
            assert!(apply(&mut preferences, action));
            assert_eq!(preferences, original);
        }
        let mut preferences = original.clone();
        assert!(apply(&mut preferences, "floating-vertical"));
        assert_eq!(preferences.layout, FloatingLayout::Vertical);
        assert!(apply(&mut preferences, "floating-horizontal"));
        assert_eq!(preferences, original);
    }

    #[test]
    fn unrelated_actions_and_invalid_values_leave_preferences_unchanged() {
        let original = FloatingPreferences::default();
        for action in [
            "floating-reset",
            "floating-settings",
            "floating-font-unknown",
            "floating-background-unknown",
            "floating-opacity-75",
            "floating-opacity-0",
            "floating-opacity-101",
            "floating-opacity-256",
            "floating-opacity-",
            "floating-unknown",
        ] {
            let mut preferences = original.clone();
            assert!(!apply(&mut preferences, action), "action={action}");
            assert_eq!(preferences, original, "action={action}");
        }
    }
}
