//! The design system: CSS, card components and the auto-hiding capability card.

pub mod card;
pub mod feature_card;
pub mod sensor_card;

pub use feature_card::FeatureCard;
pub use sensor_card::SensorCard;

use gtk4::CssProvider;

pub const STYLE_CSS: &str = r#"
/* The Windows toolkit's accent is RGB(255,33,33). libadwaita 1.9's Rust binding
   only exposes the accent colour as a read-only property, so it is set here at
   the CSS layer instead. This is the single source of truth. */
@define-color accent_bg_color #ff2121;
@define-color accent_color #ff2121;

/* ---------------------------------------------------------------- cards --- */

.llt-card {
    background-color: @card_bg_color;
    border: 1px solid @card_shade_color;
    border-radius: 8px;
    padding: 10px 12px;
    margin-bottom: 8px;
}

.llt-card-title {
    font-weight: 600;
    font-size: 14px;
}

.llt-card-subtitle {
    font-size: 12px;
    opacity: 0.65;
}

.llt-card-note {
    font-size: 11px;
    opacity: 0.55;
}

/* The card icon, matching the reference's 24px symbol beside the title. */
.llt-card-icon {
    opacity: 0.75;
    margin-right: 10px;
}

/* Card status rows, the reference's Info / Warning / Error / Success lines.
   Each is a 12px icon and a coloured message. */
.llt-card-status {
    margin-top: 4px;
}

.llt-card-status-text {
    font-size: 11px;
}

.llt-status-info {
    /* Deliberately not the accent: the accent is red here, and a red info row
       reads as an error. Information should not look like a warning. */
    color: #62a0ea;
}

.llt-status-warning {
    color: @warning_color;
}

.llt-status-error {
    color: @error_color;
}

.llt-status-success {
    color: @success_color;
}

/* The compatibility report on the About page. */
.llt-compat-name {
    font-size: 12px;
}

.llt-ok {
    font-size: 11px;
    opacity: 0.45;
    font-family: monospace;
}

.llt-missing {
    font-size: 11px;
    opacity: 0.6;
    font-style: italic;
}

/* One line at the foot of a page naming what this machine lacks. */
.llt-page-footnote {
    font-size: 12px;
    opacity: 0.5;
    margin: 16px 0 0 0;
}

/* The compact 40x6 bar from the reference SensorsControlV2. */
.llt-meter {
    min-width: 40px;
    min-height: 6px;
}

/* The 24-zone keyboard shape on the Lighting page: a framed panel with the
   zone rows staggered inside it, so it reads as a keyboard. */
.llt-keyboard {
    background-color: alpha(currentColor, 0.05);
    border-radius: 6px;
    padding: 8px;
}

/* The reference's data bars are blue even though its brand accent is red: the
   accent marks interactive things (buttons, selected rows) and the bar marks a
   measurement. Using the accent for both made every reading look like an alert,
   which is why the first version read as cheap. */
@define-color llt_meter_fill #3584e4;
@define-color llt_meter_track alpha(currentColor, 0.13);

.llt-meter trough {
    min-height: 6px;
    min-width: 40px;
    border-radius: 3px;
    background-color: @llt_meter_track;
}

.llt-meter progress {
    min-height: 6px;
    border-radius: 3px;
    background-image: linear-gradient(to right,
        alpha(@llt_meter_fill, 0.85), @llt_meter_fill);
}

.llt-value {
    font-weight: 500;
    font-size: 12px;
}

.llt-subheading {
    font-size: 15px;
    font-weight: 600;
    opacity: 0.85;
    margin: 18px 0 8px 0;
}

.llt-group-title {
    font-size: 24px;
    font-weight: 500;
    margin: 16px 0 24px 0;
}

/* ------------------------------------------------------------- sensors ---
   Ported from the reference's SensorsControlV2.xaml SensorCardStyle /
   CardTitleStyle / DataLabelStyle / DataValueStyle. The 40x6 accent meter is
   the detail that makes a row scannable without reading the number. */
.llt-sensor-card {
    background-color: @card_bg_color;
    border: 1px solid @card_shade_color;
    border-radius: 8px;
    padding: 12px 10px;
    margin: 0 8px 8px 0;
}

.llt-sensor-title {
    font-weight: 600;
    font-size: 14px;
    margin-bottom: 12px;
}

.llt-sensor-detail {
    font-size: 11px;
    opacity: 0.45;
}

.llt-sensor-label {
    font-size: 12px;
    opacity: 0.65;
}

.llt-sensor-value {
    font-size: 12px;
    font-weight: 500;
}

.llt-sensor-grid {
    /* The reference wraps its cards and lets the container decide columns. */
    margin: 0;
}

/* --------------------------------------------------------------- battery ---
   The reference's BatteryPage hero: a large battery icon beside a 32px
   percentage and a secondary status line. */
.llt-hero-percent {
    font-size: 32px;
    font-weight: 500;
}

.llt-hero-status {
    font-size: 16px;
    opacity: 0.7;
}

/* ------------------------------------------------------- unavailable ---- */

.llt-unavailable {
    opacity: 0.55;
    font-size: 11px;
    font-style: italic;
}

/* Inline outcome of a write: grey while applying, red on failure. */
.llt-status {
    font-size: 11px;
    margin-top: 4px;
    color: @warning_color;
}

.llt-status.applying {
    opacity: 0.6;
}
"#;

/// Install the stylesheet and follow the system's dark/light preference.
pub fn init() {
    let provider = CssProvider::new();
    provider.load_from_string(STYLE_CSS);

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    apply_color_scheme(legiontoolkit::settings::current().color_scheme);
}

/// Apply a stored preference. Called at startup and whenever Settings changes.
pub fn apply_color_scheme(scheme: legiontoolkit::settings::ColorScheme) {
    use legiontoolkit::settings::ColorScheme;
    adw::StyleManager::default().set_color_scheme(match scheme {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
    });
}

/// Cycle System → Light → Dark → System, mirroring the reference's theme toggle.
pub fn cycle_color_scheme() {
    use legiontoolkit::settings::ColorScheme;
    let style = adw::StyleManager::default();
    let next = match style.color_scheme() {
        adw::ColorScheme::Default => ColorScheme::Light,
        adw::ColorScheme::ForceLight => ColorScheme::Dark,
        _ => ColorScheme::System,
    };
    let _ = legiontoolkit::settings::update(|s| s.color_scheme = next);
    apply_color_scheme(next);
}
