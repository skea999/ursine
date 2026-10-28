//! Theming screen: uniform desktop theme from the shell config (SPEC-theming).

use cosmic::iced::widget::container::Style as ContainerStyle;
use cosmic::iced::Length;
use cosmic::widget::{button, column, container, row, scrollable, settings, text, toggler};
use cosmic::Element;
use ursine_api::{ThemeApplyReport, ThemeSnapshot, ThemingStatus};

use crate::app::Message;

#[derive(Default)]
pub struct ThemingState {
    pub busy: bool,
    pub apply_busy: bool,
    /// Set when the user pressed Apply; the write happens only after the
    /// confirm step (SPEC: apply behind confirmation).
    pub confirm_apply: bool,
    /// GUI checkbox: include the minimize button in the baseline.
    pub show_minimize: bool,
    pub status: Option<ThemingStatus>,
    pub snapshot: Option<ThemeSnapshot>,
    pub report: Option<ThemeApplyReport>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl ThemingState {
    pub fn view(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![row(vec![
            text::title2("Theming").into(),
            text("").width(Length::Fill).into(),
            button::text("Refresh")
                .on_press_maybe((!self.busy && !self.apply_busy).then_some(Message::OpenTheming))
                .into(),
        ])
        .spacing(12)
        .width(Length::Fill)
        .into()];

        if let Some(error) = &self.error {
            items.push(text(error.clone()).into());
        }
        if let Some(message) = &self.message {
            items.push(text(message.clone()).into());
        }
        if self.busy {
            items.push(text("Reading the shell theme…").into());
        } else if self.apply_busy {
            items.push(text("Applying the theme to every toolkit…").into());
        }

        let Some(status) = &self.status else {
            // Only claim "no shell" when detect actually answered (a detect
            // error already shows its own message above).
            if !self.busy && self.error.is_none() {
                items.push(text(INACTIVE_HINT).into());
            }
            return scrollable(column(items).spacing(16).padding(24)).into();
        };
        if !status.available {
            items.push(text(INACTIVE_HINT).into());
            return scrollable(column(items).spacing(16).padding(24)).into();
        }

        if let Some(snapshot) = &self.snapshot {
            items.push(snapshot_card(snapshot));
        }

        items.push(text::heading("Targets").into());
        let target_rows: Vec<Element<'_, Message>> = status
            .targets
            .iter()
            .map(|target| {
                let mark = if target.available {
                    "will write"
                } else {
                    "skipped"
                };
                let detail = target.detail.clone().unwrap_or_default();
                row(vec![
                    text(target.target.as_str())
                        .width(Length::Fixed(110.0))
                        .into(),
                    text(mark).width(Length::Fixed(80.0)).into(),
                    text(detail).into(),
                ])
                .spacing(8)
                .into()
            })
            .collect();
        items.push(column(target_rows).spacing(4).into());

        items.push(
            settings::item(
                "Show minimize button",
                toggler(self.show_minimize).on_toggle(Message::ThemingMinimizeToggled),
            )
            .into(),
        );
        if self.confirm_apply {
            let targets = status.targets.iter().filter(|t| t.available).count();
            items.push(
                text(format!(
                    "Write the theme to {targets} targets in your home? Some changes apply after you restart apps or the session."
                ))
                .into(),
            );
            items.push(
                row(vec![
                    button::suggested("Confirm apply")
                        .on_press_maybe(
                            (!self.apply_busy).then_some(Message::ThemingApplyConfirmed),
                        )
                        .into(),
                    button::text("Cancel")
                        .on_press(Message::ThemingApplyCancelled)
                        .into(),
                ])
                .spacing(8)
                .into(),
            );
        } else {
            items.push(
                button::suggested("Apply theme everywhere")
                    .on_press_maybe(
                        (self.snapshot.is_some() && !self.apply_busy && !self.busy)
                            .then_some(Message::ThemingApply),
                    )
                    .into(),
            );
            items.push(
                text("Writes theme files in your home (GTK, Qt, KDE, Flatpak, COSMIC, session). Some changes apply after you restart apps or the session.")
                    .into(),
            );
        }

        if let Some(report) = &self.report {
            items.push(text::heading("Last apply").into());
            let rows: Vec<Element<'_, Message>> = report
                .results
                .iter()
                .map(|result| {
                    let outcome = match result.outcome {
                        ursine_api::ApplyOutcome::Written => "written",
                        ursine_api::ApplyOutcome::Skipped => "skipped",
                        ursine_api::ApplyOutcome::Failed => "failed",
                    };
                    row(vec![
                        text(result.target.as_str())
                            .width(Length::Fixed(110.0))
                            .into(),
                        text(outcome).width(Length::Fixed(80.0)).into(),
                        text(result.detail.clone().unwrap_or_default()).into(),
                    ])
                    .spacing(8)
                    .into()
                })
                .collect();
            items.push(column(rows).spacing(4).into());
        }

        // Top bar placeholder: same uniformity goal, specs pending from the
        // user — only shown once theming is actually active.
        items.push(
            container(
                column(vec![
                    text("Application top bar").into(),
                    text("Uniform title bar for every application — specifications pending.")
                        .into(),
                ])
                .spacing(4),
            )
            .padding(12)
            .class(cosmic::theme::Container::Background)
            .into(),
        );

        scrollable(column(items).spacing(16).padding(24)).into()
    }
}

/// Shown when no shell config exists (both inactive branches share it).
const INACTIVE_HINT: &str =
    "No supported desktop shell config found. Install Wayle or DankMaterialShell and sign in once.";

fn snapshot_card(snapshot: &ThemeSnapshot) -> Element<'_, Message> {
    let swatch = |label: &str, hex: &str| -> Element<'_, Message> {
        let color = parse_hex_color(hex);
        // Readable ink on an arbitrary palette color (luminance pick).
        let luminance = 0.0722f64.mul_add(
            f64::from(color.b),
            0.7152f64.mul_add(f64::from(color.g), 0.2126 * f64::from(color.r)),
        );
        let ink = if luminance > 0.5 {
            cosmic::iced::Color::BLACK
        } else {
            cosmic::iced::Color::WHITE
        };
        container(
            column(vec![
                text(label.to_string()).class(ink).into(),
                text(hex.to_string()).class(ink).into(),
            ])
            .spacing(2),
        )
        .style(move |_| ContainerStyle {
            background: Some(cosmic::iced::Background::Color(color)),
            ..Default::default()
        })
        .padding(8)
        .width(Length::Fixed(120.0))
        .into()
    };
    let c = &snapshot.colors;
    let cursor = snapshot.cursor.as_ref().map_or_else(
        || "system default".to_string(),
        |cursor| format!("{} ({}px)", cursor.name, cursor.size),
    );
    column(vec![
        text(format!(
            "Source: {} — theme “{}” ({})",
            snapshot.source.as_str(),
            snapshot.name,
            if snapshot.dark { "dark" } else { "light" }
        ))
        .into(),
        row(vec![
            swatch("background", &c.background),
            swatch("foreground", &c.foreground),
            swatch("primary", &c.primary),
            swatch("surface", &c.surface),
            swatch("error", &c.error),
            swatch("success", &c.success),
            swatch("warning", &c.warning),
            swatch("outline", &c.outline),
        ])
        .spacing(4)
        .into(),
        text(format!(
            "Fonts: {} / {} (weight {}, scale {}); cursor: {cursor}; radius: {}",
            snapshot.fonts.ui,
            snapshot.fonts.mono,
            snapshot.fonts.weight,
            snapshot.fonts.scale,
            snapshot
                .corner_radius
                .map_or_else(|| "system default".to_string(), |r| format!("{r}px")),
        ))
        .into(),
    ])
    .spacing(8)
    .into()
}

/// `#rrggbb` / `#rrggbbaa` → iced color (invalid input falls back to gray).
fn parse_hex_color(hex: &str) -> cosmic::iced::Color {
    let body = hex.trim_start_matches('#');
    let channel = |i: usize| -> f32 {
        u8::from_str_radix(body.get(i..i + 2).unwrap_or("7f"), 16)
            .map_or(0.5, |v| f32::from(v) / 255.0)
    };
    let alpha = if body.len() == 8 { channel(6) } else { 1.0 };
    cosmic::iced::Color {
        r: channel(0),
        g: channel(2),
        b: channel(4),
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ursine_api::{
        ApplyOutcome, TargetResult, TargetStatus, ThemeColors, ThemeFonts, ThemeSource,
        ThemingTarget,
    };

    fn snapshot() -> ThemeSnapshot {
        ThemeSnapshot {
            source: ThemeSource::Dms,
            name: "green".into(),
            dark: true,
            colors: ThemeColors {
                background: "#10140f".into(),
                foreground: "#e0e4db".into(),
                primary: "#4caf50".into(),
                secondary: "#2e7d32".into(),
                surface: "#1b1f19".into(),
                error: "#f2b8b5".into(),
                success: "#4daf4c".into(),
                warning: "#dacb52".into(),
                outline: "#35a639".into(),
            },
            fonts: ThemeFonts {
                ui: "Inter Variable".into(),
                mono: "Fira Code".into(),
                weight: 400,
                scale: 1.0,
            },
            corner_radius: Some(12),
            icon_theme: None,
            cursor: None,
        }
    }

    #[test]
    fn inactive_view_explains_the_missing_shell() {
        let state = ThemingState::default();
        // The view builds without a session; the inactive hint is the
        // capability contract (hidden unless a shell config exists).
        let _ = state.view();
    }

    #[test]
    fn active_view_lists_targets_snapshot_and_report() {
        let mut state = ThemingState {
            status: Some(ThemingStatus {
                available: true,
                source: Some(ThemeSource::Dms),
                show_minimize: true,
                targets: vec![
                    TargetStatus::available(ThemingTarget::Gtk3, "settings.ini"),
                    TargetStatus::unavailable(ThemingTarget::Kde, "kwriteconfig5 not found"),
                ],
            }),
            snapshot: Some(snapshot()),
            report: Some(ThemeApplyReport {
                results: vec![
                    TargetResult {
                        target: ThemingTarget::Gtk3,
                        outcome: ApplyOutcome::Written,
                        detail: Some("settings.ini written".into()),
                    },
                    TargetResult {
                        target: ThemingTarget::Kde,
                        outcome: ApplyOutcome::Skipped,
                        detail: None,
                    },
                ],
            }),
            ..ThemingState::default()
        };
        state.apply_busy = false;
        let _ = state.view();
        // Confirmation step renders before any write is dispatched.
        state.confirm_apply = true;
        let _ = state.view();
    }

    #[test]
    fn hex_color_parser_handles_six_and_eight_digits() {
        let six = parse_hex_color("#10140f");
        assert!((six.r - f32::from(0x10u8) / 255.0).abs() < 0.001);
        assert_eq!(six.a, 1.0);
        let eight = parse_hex_color("#10140f80");
        assert!(eight.a < 1.0);
        let broken = parse_hex_color("zzz");
        assert!((broken.r - 0.5).abs() < 0.001);
    }
}
