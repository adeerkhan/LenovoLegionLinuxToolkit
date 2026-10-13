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

/* The compact 40x6 bar from the reference SensorsControlV2. */
.llt-meter {
    min-width: 40px;
    min-height: 6px;
}

.llt-meter trough {
    min-height: 6px;
    border-radius: 3px;
}

.llt-meter progress {
    min-height: 6px;
    border-radius: 3px;
    background-color: @accent_bg_color;
}

.llt-value {
    font-weight: 500;
    font-size: 12px;
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

    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
}

/// Cycle System → Light → Dark → System, mirroring the reference's theme
/// toggle.
pub fn cycle_color_scheme() {
    let style = adw::StyleManager::default();
    style.set_color_scheme(match style.color_scheme() {
        adw::ColorScheme::Default => adw::ColorScheme::ForceLight,
        adw::ColorScheme::ForceLight => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
}
