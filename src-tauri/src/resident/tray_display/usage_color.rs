//! Shared usage tones; native adapters own the actual glyph rendering.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageTone {
    #[default]
    Normal,
    Green,
    Yellow,
    Blue,
    Warning,
    Critical,
}

impl UsageTone {
    #[cfg(any(test, windows, target_os = "macos"))]
    pub fn rgb(self, foreground: [u8; 3]) -> [u8; 3] {
        // Darker hues stay legible on light taskbars; lighter hues serve dark
        // surfaces. Normal and unavailable readings keep the system foreground.
        let dark =
            u16::from(foreground[0]) + u16::from(foreground[1]) + u16::from(foreground[2]) > 384;
        match (self, dark) {
            (Self::Normal, _) => foreground,
            (Self::Green, false) => [24, 120, 62],
            (Self::Green, true) => [97, 213, 145],
            (Self::Yellow, false) => [133, 101, 0],
            (Self::Yellow, true) => [240, 207, 94],
            (Self::Blue, false) => [27, 99, 190],
            (Self::Blue, true) => [117, 180, 255],
            (Self::Warning, false) => [166, 77, 0],
            (Self::Warning, true) => [255, 190, 92],
            (Self::Critical, false) => [194, 34, 42],
            (Self::Critical, true) => [255, 112, 117],
        }
    }
}

impl From<super::super::color_rules::DisplayColor> for UsageTone {
    fn from(color: super::super::color_rules::DisplayColor) -> Self {
        use super::super::color_rules::DisplayColor;
        match color {
            DisplayColor::Default => Self::Normal,
            DisplayColor::Green => Self::Green,
            DisplayColor::Yellow => Self::Yellow,
            DisplayColor::Orange => Self::Warning,
            DisplayColor::Red => Self::Critical,
            DisplayColor::Blue => Self::Blue,
        }
    }
}

/// Classify valid displayed values; hold descending boundaries briefly to avoid flicker.
pub fn next_rule(
    previous: Option<usize>,
    value: Option<u16>,
    metric: &super::super::color_rules::MetricColorRule,
) -> Option<usize> {
    let value = value.filter(|_| metric.enabled)?;
    let candidate = metric.rules.iter().position(|rule| rule.matches(value))?;
    if let Some(previous) = previous.filter(|index| *index < metric.rules.len()) {
        let (from, _) = metric.rules[previous].bounds();
        let (_, candidate_end) = metric.rules[candidate].bounds();
        let margin = if metric.id == super::super::preference_schema::DisplayMetricId::FanSpeed {
            100
        } else if super::super::color_rules::temperature(metric.id) {
            2
        } else {
            3
        };
        // Hold only adjacent descending boundaries; gaps always use the default color.
        if from > value && candidate_end == Some(from) && value > from.saturating_sub(margin) {
            return Some(previous);
        }
    }
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resident::{color_rules::MetricColorRule, preference_schema::DisplayMetricId};
    #[test]
    fn gaps_clear_previous_colors_and_rule_order_does_not_set_priority() {
        let mut rule = MetricColorRule::defaults(DisplayMetricId::Cpu);
        rule.rules.remove(1);
        rule.rules.reverse();
        assert_eq!(next_rule(None, Some(90), &rule), Some(0));
        assert_eq!(next_rule(Some(0), Some(89), &rule), None);
        assert_eq!(next_rule(None, Some(69), &rule), Some(1));
        rule.rules.clear();
        assert_eq!(next_rule(Some(0), Some(99), &rule), None);
    }

    #[test]
    fn zero_is_valid_and_missing_values_clear_hysteresis() {
        let rule = MetricColorRule::defaults(DisplayMetricId::Cpu);
        assert_eq!(next_rule(None, Some(0), &rule), Some(0));
        assert_eq!(next_rule(None, Some(90), &rule), Some(2));
        assert_eq!(next_rule(Some(2), Some(89), &rule), Some(2));
        assert_eq!(next_rule(Some(2), Some(87), &rule), Some(1));
        assert_eq!(next_rule(Some(2), None, &rule), None);
        let mut rule = MetricColorRule::defaults(DisplayMetricId::CpuTemperature);
        rule.rules[1] = crate::resident::color_rules::ColorRule::Range {
            from: 60,
            to: 120,
            color: crate::resident::color_rules::DisplayColor::Orange,
        };
        rule.rules[2] = crate::resident::color_rules::ColorRule::AtLeast {
            value: 120,
            color: crate::resident::color_rules::DisplayColor::Red,
        };
        assert_eq!(next_rule(None, Some(120), &rule), Some(2));
        assert_eq!(next_rule(Some(2), Some(119), &rule), Some(2));
        assert_eq!(next_rule(Some(2), Some(118), &rule), Some(1));
        rule.enabled = false;
        assert_eq!(next_rule(Some(2), Some(120), &rule), None);
    }
    #[test]
    fn palette_adapts_to_light_and_dark_foregrounds() {
        for color in [
            UsageTone::Green,
            UsageTone::Yellow,
            UsageTone::Warning,
            UsageTone::Critical,
            UsageTone::Blue,
        ] {
            assert_ne!(color.rgb([32; 3]), color.rgb([245; 3]));
        }
        assert_eq!(UsageTone::Normal.rgb([32; 3]), [32; 3]);
    }
}
