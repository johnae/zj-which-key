//! zj-which-key: a delayed, which-key style keybinding popup for Zellij.
//!
//! The plugin runs in two roles from a single wasm binary:
//!
//! - **Controller** (loaded in the background via `load_plugins`): has no pane of
//!   its own. It watches mode changes, and after a short idle delay spawns the
//!   popup. One controller exists per client, so per-client mode state stays
//!   isolated automatically.
//! - **Popup** (spawned by the controller as a floating, non-selectable pane):
//!   renders the current mode's keybindings, sizes itself to its content in a
//!   corner, and closes itself when you return to the base mode.
//!
//! The role is selected by the `role` config key (`controller` by default,
//! `popup` for the spawned instance).

use ansi_term::Colour;
use std::collections::BTreeMap;
use std::collections::HashMap;
use zellij_tile::prelude::actions::Action;
use zellij_tile::prelude::*;

const DEFAULT_DELAY_SECS: f64 = 0.4;
const DEFAULT_MAX_HEIGHT_PCT: usize = 40;

/// Margin between the popup and the screen edge, in cells.
const MARGIN: usize = 1;
/// Hard cap on the popup's inner content width.
const MAX_INNER_WIDTH: usize = 64;
/// Hard cap on the width of the keys column.
const KEYS_COL_MAX: usize = 18;
/// Smallest pane we'll ever ask for (border included).
const MIN_BOX_ROWS: usize = 4;

#[derive(Default, PartialEq, Clone, Copy)]
enum Role {
    #[default]
    Controller,
    Popup,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Position {
    #[default]
    BottomRight,
    BottomLeft,
}

#[derive(Default)]
struct State {
    role: Role,
    position: Position,
    mode_info: ModeInfo,

    auto_show: bool,
    delay_secs: f64,
    max_height_pct: usize,

    permissions_granted: bool,
    own_id: u32,

    /// Controller: whether a popup instance is currently alive.
    popup_visible: bool,
    /// Display area of the focused tab, learned from `TabUpdate`.
    display_rows: usize,
    display_cols: usize,
    /// Popup: the last coordinates we asked for, to avoid redundant resizes.
    last_coords: Option<(usize, usize, usize, usize)>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.parse_config(configuration);
        self.own_id = get_plugin_ids().plugin_id;

        match self.role {
            Role::Popup => {
                // Stay out of the focus order so we never steal keys from the mode.
                set_selectable(false);
                request_permission(&[
                    PermissionType::ReadApplicationState,
                    PermissionType::ChangeApplicationState,
                ]);
                subscribe(&[
                    EventType::ModeUpdate,
                    EventType::TabUpdate,
                    EventType::PermissionRequestResult,
                ]);
            }
            Role::Controller => {
                request_permission(&[
                    PermissionType::ReadApplicationState,
                    PermissionType::ChangeApplicationState,
                    PermissionType::MessageAndLaunchOtherPlugins,
                ]);
                subscribe(&[
                    EventType::ModeUpdate,
                    EventType::TabUpdate,
                    EventType::Timer,
                    EventType::PermissionRequestResult,
                ]);
            }
        }
    }

    fn update(&mut self, event: Event) -> bool {
        match self.role {
            Role::Popup => self.update_popup(event),
            Role::Controller => self.update_controller(event),
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if self.role != Role::Popup {
            return;
        }
        self.render_popup(rows, cols);
    }
}

impl State {
    fn parse_config(&mut self, config: BTreeMap<String, String>) {
        self.role = match config.get("role").map(String::as_str) {
            Some("popup") => Role::Popup,
            _ => Role::Controller,
        };
        self.position = match config.get("position").map(String::as_str) {
            Some("bottom-left") => Position::BottomLeft,
            _ => Position::BottomRight,
        };
        self.auto_show = config
            .get("auto_show")
            .map(|s| s == "true")
            .unwrap_or(true);
        self.delay_secs = config
            .get("delay_secs")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_DELAY_SECS);
        self.max_height_pct = config
            .get("max_height_pct")
            .and_then(|s| s.parse().ok())
            .filter(|p| *p > 0 && *p <= 100)
            .unwrap_or(DEFAULT_MAX_HEIGHT_PCT);
    }

    fn base_mode(&self) -> InputMode {
        self.mode_info.base_mode.unwrap_or(InputMode::Normal)
    }

    fn is_base_mode(&self) -> bool {
        self.mode_info.mode == self.base_mode()
    }

    // ---- Controller ----------------------------------------------------------

    fn update_controller(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.permissions_granted = true;
            }
            Event::TabUpdate(tabs) => {
                self.update_display_area(&tabs);
            }
            Event::ModeUpdate(mode_info) => {
                self.mode_info = mode_info;
                if !self.permissions_granted || !self.auto_show {
                    return false;
                }
                if self.is_base_mode() {
                    // The popup closes itself on base mode; just track that.
                    self.popup_visible = false;
                } else if !self.popup_visible {
                    // Arm the idle delay; we spawn when the timer fires.
                    set_timeout(self.delay_secs);
                }
            }
            Event::Timer(_)
                if self.permissions_granted
                    && self.auto_show
                    && !self.is_base_mode()
                    && !self.popup_visible =>
            {
                self.spawn_popup();
                self.popup_visible = true;
            }
            _ => {}
        }
        false
    }

    fn spawn_popup(&self) {
        let mut config = BTreeMap::new();
        config.insert("role".to_string(), "popup".to_string());
        config.insert(
            "max_height_pct".to_string(),
            self.max_height_pct.to_string(),
        );
        config.insert(
            "position".to_string(),
            match self.position {
                Position::BottomLeft => "bottom-left".to_string(),
                Position::BottomRight => "bottom-right".to_string(),
            },
        );

        let mut message = MessageToPlugin::new("spawn_popup")
            .with_plugin_url("zellij:OWN_URL")
            .with_plugin_config(config)
            .new_plugin_instance_should_have_pane_title("which-key");

        if let Some(coords) = self.corner_coords() {
            message = message.with_floating_pane_coordinates(coords);
        }
        pipe_message_to_plugin(message);
    }

    // ---- Popup ---------------------------------------------------------------

    fn update_popup(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.permissions_granted = true;
                set_selectable(false);
                false
            }
            Event::TabUpdate(tabs) => {
                self.update_display_area(&tabs);
                self.reposition();
                false
            }
            Event::ModeUpdate(mode_info) => {
                let was_base = self.is_base_mode();
                self.mode_info = mode_info;
                if !was_base && self.is_base_mode() {
                    // Returned to base mode: vanish.
                    close_self();
                    return false;
                }
                self.reposition();
                true
            }
            _ => false,
        }
    }

    /// Resize/move ourselves to hug the current content in the chosen corner.
    /// No-op when the target box is unchanged, so mode/tab churn stays quiet.
    fn reposition(&mut self) {
        let Some(box_) = self.corner_box() else {
            return;
        };
        if self.last_coords == Some(box_) {
            return;
        }
        self.last_coords = Some(box_);
        let (x, y, w, h) = box_;
        if let Some(coords) = floating_coords(x, y, w, h) {
            change_floating_panes_coordinates(vec![(PaneId::Plugin(self.own_id), coords)]);
        }
    }

    fn render_popup(&mut self, pane_rows: usize, pane_cols: usize) {
        // Prefer the known display area; fall back to our pane size before the
        // first TabUpdate arrives.
        let display_cols = if self.display_cols > 0 {
            self.display_cols
        } else {
            pane_cols + 2
        };
        let display_rows = if self.display_rows > 0 {
            self.display_rows
        } else {
            pane_rows + 2
        };

        let entries = self.entries();
        let layout = compute_layout(&entries, display_cols, display_rows, self.max_height_pct);

        let header = Colour::Fixed(252).bold();
        let keys_style = Colour::Fixed(75).bold();
        let label_style = Colour::Fixed(250).normal();
        let accent_back = Colour::Fixed(114).normal();
        let accent_switch = Colour::Fixed(180).normal();
        let dim = Colour::Fixed(244).normal();

        println!(
            "{}",
            header.paint(format!("{:?} mode", self.mode_info.mode))
        );

        for entry in entries.iter().take(layout.visible) {
            let keys = pad_right(
                &truncate_to_width(&entry.keys_str(), layout.keys_col),
                layout.keys_col,
            );
            let label = truncate_to_width(&entry.label, layout.label_col);
            let label_style = if entry.label.starts_with("Back to") {
                accent_back
            } else if entry.label.ends_with(" mode") {
                accent_switch
            } else {
                label_style
            };
            println!(
                "{}  {}",
                keys_style.paint(keys),
                label_style.paint(label)
            );
        }

        if layout.overflow > 0 {
            println!("{}", dim.paint(format!("+{} more", layout.overflow)));
        }
    }

    // ---- Shared sizing -------------------------------------------------------

    fn entries(&self) -> Vec<Entry> {
        let base = self.base_mode();
        // Anything also bound in the base mode is a global (focus/resize/etc.)
        // that works everywhere; hide those from the per-mode popup.
        let globals: std::collections::HashSet<String> = self
            .mode_info
            .get_keybinds_for_mode(base)
            .iter()
            .map(|(key, actions)| binding_signature(key, actions, base))
            .collect();
        group_bindings(
            &self.mode_info.get_mode_keybinds(),
            self.mode_info.mode,
            base,
            &globals,
        )
    }

    /// A content-sized `(x, y, width, height)` box tucked into the chosen
    /// corner of a `cols`x`rows` display area.
    fn corner_box_in(&self, cols: usize, rows: usize) -> (usize, usize, usize, usize) {
        let entries = self.entries();
        let layout = compute_layout(&entries, cols, rows, self.max_height_pct);
        let x = match self.position {
            Position::BottomRight => cols.saturating_sub(layout.pane_cols + MARGIN),
            Position::BottomLeft => MARGIN,
        };
        let y = rows.saturating_sub(layout.pane_rows + MARGIN);
        (x, y, layout.pane_cols, layout.pane_rows)
    }

    /// The corner box for the known display area, or `None` if it isn't known yet.
    fn corner_box(&self) -> Option<(usize, usize, usize, usize)> {
        if self.display_cols == 0 || self.display_rows == 0 {
            return None;
        }
        Some(self.corner_box_in(self.display_cols, self.display_rows))
    }

    /// Spawn coordinates for the controller, falling back to a guess before the
    /// display area is known.
    fn corner_coords(&self) -> Option<FloatingPaneCoordinates> {
        let (x, y, w, h) = self
            .corner_box()
            .unwrap_or_else(|| self.corner_box_in(200, 50));
        floating_coords(x, y, w, h)
    }

    fn update_display_area(&mut self, tabs: &[TabInfo]) {
        if let Some(tab) = tabs.iter().find(|t| t.active) {
            self.display_rows = tab.display_area_rows;
            self.display_cols = tab.display_area_columns;
        }
    }
}

/// One row of the popup: every key bound to a single action, plus its label.
struct Entry {
    priority: u8,
    keys: Vec<String>,
    label: String,
}

impl Entry {
    fn keys_str(&self) -> String {
        self.keys.join(" ")
    }
}

struct Layout {
    pane_cols: usize,
    pane_rows: usize,
    keys_col: usize,
    label_col: usize,
    visible: usize,
    overflow: usize,
}

/// Group a mode's keybindings by action, ordered by priority, dropping noise.
/// Group a mode's keybindings by action, dropping noise and any binding whose
/// signature is in `exclude` (used to hide globals from the per-mode popup).
fn group_bindings(
    binds: &[(KeyWithModifier, Vec<Action>)],
    mode: InputMode,
    base_mode: InputMode,
    exclude: &std::collections::HashSet<String>,
) -> Vec<Entry> {
    let mut order: Vec<String> = Vec::new();
    let mut by_label: HashMap<String, Entry> = HashMap::new();

    for (key, actions) in binds {
        if is_noise(actions) || exclude.contains(&binding_signature(key, actions, base_mode)) {
            continue;
        }
        let label = format_action(actions, base_mode);
        let entry = by_label.entry(label.clone()).or_insert_with(|| {
            order.push(label.clone());
            Entry {
                priority: action_priority(actions, mode, base_mode),
                keys: Vec::new(),
                label,
            }
        });
        let key_str = format_key(key);
        if !entry.keys.contains(&key_str) {
            entry.keys.push(key_str);
        }
    }

    let mut entries: Vec<Entry> = order
        .into_iter()
        .filter_map(|label| by_label.remove(&label))
        .collect();
    entries.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.label.cmp(&b.label)));
    entries
}

fn floating_coords(x: usize, y: usize, w: usize, h: usize) -> Option<FloatingPaneCoordinates> {
    FloatingPaneCoordinates::new(
        Some(x.to_string()),
        Some(y.to_string()),
        Some(w.to_string()),
        Some(h.to_string()),
        Some(true),
    )
}

fn compute_layout(
    entries: &[Entry],
    display_cols: usize,
    display_rows: usize,
    max_height_pct: usize,
) -> Layout {
    let keys_col = entries
        .iter()
        .map(|e| display_width(&e.keys_str()))
        .max()
        .unwrap_or(0)
        .clamp(1, KEYS_COL_MAX);

    // Leave room for the border (2) and one cell of padding on each side (2).
    let max_inner = MAX_INNER_WIDTH.min(display_cols.saturating_sub(2 * MARGIN + 4));
    let raw_label = entries
        .iter()
        .map(|e| display_width(&e.label))
        .max()
        .unwrap_or(1);
    let label_col = raw_label.min(max_inner.saturating_sub(keys_col + 2)).max(1);

    let inner = keys_col + 2 + label_col;
    let pane_cols = (inner + 4)
        .min(display_cols.saturating_sub(2 * MARGIN))
        .max(8);

    let cap_rows = (display_rows.saturating_mul(max_height_pct) / 100).max(MIN_BOX_ROWS);
    // Subtract the border (2) and the mode header (1) to get body capacity.
    let body_avail = cap_rows.saturating_sub(3).max(1);

    let (visible, overflow) = if entries.len() <= body_avail {
        (entries.len(), 0)
    } else {
        // One body line goes to the "+N more" indicator.
        let visible = body_avail.saturating_sub(1).max(1);
        (visible, entries.len() - visible)
    };

    let content_rows = 1 + visible + usize::from(overflow > 0);
    let pane_rows = (content_rows + 2).max(MIN_BOX_ROWS);

    Layout {
        pane_cols,
        pane_rows,
        keys_col,
        label_col,
        visible,
        overflow,
    }
}

/// Visible width of a string. Our key/label glyphs are all single-width, so a
/// char count is exact here and avoids a unicode-width dependency.
fn display_width(s: &str) -> usize {
    s.chars().count()
}

/// Truncate to `max` columns with an ellipsis, never splitting a char.
fn truncate_to_width(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if display_width(s) <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    let mut out = s.to_string();
    if w < width {
        out.extend(std::iter::repeat_n(' ', width - w));
    }
    out
}

/// Stable identity for a `key -> action` binding, used to match the same
/// binding across modes (e.g. to detect globals present in the base mode).
fn binding_signature(key: &KeyWithModifier, actions: &[Action], base_mode: InputMode) -> String {
    format!("{}\t{}", format_key(key), format_action(actions, base_mode))
}

fn is_noise(actions: &[Action]) -> bool {
    match actions.first() {
        None => true,
        // Raw "type this into the terminal" bindings carry no useful hint.
        Some(Action::Write(..)) | Some(Action::WriteChars(..)) => true,
        _ => false,
    }
}

fn format_key(key: &KeyWithModifier) -> String {
    let mut result = String::new();
    if key.has_modifiers(&[KeyModifier::Ctrl]) {
        result.push_str("Ctrl+");
    }
    if key.has_modifiers(&[KeyModifier::Alt]) {
        result.push_str("Alt+");
    }
    if key.has_modifiers(&[KeyModifier::Shift]) {
        result.push_str("Shift+");
    }
    match key.bare_key {
        BareKey::Char(c) => result.push(c),
        BareKey::F(n) => result.push_str(&format!("F{}", n)),
        BareKey::Enter => result.push('↵'),
        BareKey::Esc => result.push_str("Esc"),
        BareKey::Tab => result.push_str("Tab"),
        BareKey::Backspace => result.push_str("Bksp"),
        BareKey::Delete => result.push_str("Del"),
        BareKey::Insert => result.push_str("Ins"),
        BareKey::Home => result.push_str("Home"),
        BareKey::End => result.push_str("End"),
        BareKey::PageUp => result.push_str("PgUp"),
        BareKey::PageDown => result.push_str("PgDn"),
        BareKey::Up => result.push('↑'),
        BareKey::Down => result.push('↓'),
        BareKey::Left => result.push('←'),
        BareKey::Right => result.push('→'),
        _ => result.push('?'),
    }
    result
}

fn format_action(actions: &[Action], base_mode: InputMode) -> String {
    let Some(action) = actions.first() else {
        return "—".to_string();
    };
    match action {
        Action::Quit => "Quit zellij".to_string(),
        Action::SwitchToMode(mode) => {
            if *mode == base_mode {
                "Back to normal".to_string()
            } else {
                format!("{:?} mode", mode)
            }
        }
        Action::Resize(resize, dir) => {
            let verb = match resize {
                Resize::Increase => "Grow",
                Resize::Decrease => "Shrink",
            };
            match dir {
                Some(d) => format!("{} {:?}", verb, d),
                None => verb.to_string(),
            }
        }
        Action::MoveFocus(d) | Action::MoveFocusOrTab(d) => format!("Focus {:?}", d),
        Action::MovePane(Some(d)) => format!("Move pane {:?}", d),
        Action::MovePane(None) => "Move pane".to_string(),
        Action::NewPane(dir, ..) => match dir {
            Some(d) => format!("New pane {:?}", d),
            None => "New pane".to_string(),
        },
        Action::CloseFocus => "Close pane".to_string(),
        Action::NewTab(..) => "New tab".to_string(),
        Action::GoToTab(n) => format!("Go to tab {}", n),
        Action::GoToNextTab => "Next tab".to_string(),
        Action::GoToPreviousTab => "Previous tab".to_string(),
        Action::CloseTab => "Close tab".to_string(),
        Action::ToggleFloatingPanes => "Toggle floating".to_string(),
        Action::TogglePaneFrames => "Toggle frames".to_string(),
        Action::ToggleFocusFullscreen => "Fullscreen".to_string(),
        Action::ToggleActiveSyncTab => "Sync tab".to_string(),
        Action::ToggleTab => "Toggle tab".to_string(),
        Action::PaneNameInput(..) => "Rename pane".to_string(),
        Action::TabNameInput(..) => "Rename tab".to_string(),
        Action::UndoRenamePane => "Undo rename".to_string(),
        Action::UndoRenameTab => "Undo rename".to_string(),
        Action::HalfPageScrollDown => "Half page down".to_string(),
        Action::HalfPageScrollUp => "Half page up".to_string(),
        Action::PageScrollDown => "Page down".to_string(),
        Action::PageScrollUp => "Page up".to_string(),
        Action::ScrollDown => "Scroll down".to_string(),
        Action::ScrollUp => "Scroll up".to_string(),
        Action::ScrollToBottom => "Scroll to bottom".to_string(),
        Action::EditScrollback => "Edit scrollback".to_string(),
        Action::Detach => "Detach".to_string(),
        Action::Run(..) => "Run command".to_string(),
        Action::SwitchFocus => "Switch focus".to_string(),
        Action::MoveTab(d) => format!("Move tab {:?}", d),
        Action::NewStackedPane(..) => "New stacked pane".to_string(),
        Action::TogglePaneEmbedOrFloating => "Embed / float pane".to_string(),
        Action::NextSwapLayout => "Next layout".to_string(),
        Action::PreviousSwapLayout => "Previous layout".to_string(),
        Action::BreakPane => "Break pane to new tab".to_string(),
        Action::BreakPaneRight => "Break pane right".to_string(),
        Action::BreakPaneLeft => "Break pane left".to_string(),
        Action::ToggleGroupMarking => "Mark pane group".to_string(),
        Action::TogglePaneInGroup => "Toggle pane in group".to_string(),
        other => format!("{:?}", other),
    }
}

fn action_priority(actions: &[Action], _mode: InputMode, base_mode: InputMode) -> u8 {
    let Some(action) = actions.first() else {
        return 200;
    };
    match action {
        Action::NewPane(..) | Action::NewTab(..) | Action::NewStackedPane(..) => 10,
        Action::CloseFocus | Action::CloseTab => 11,
        Action::MoveFocus(..) | Action::MoveFocusOrTab(..) | Action::SwitchFocus => 12,
        Action::MovePane(..) => 13,
        Action::Resize(..) => 14,
        Action::BreakPane | Action::BreakPaneRight | Action::BreakPaneLeft => 16,
        Action::GoToTab(..)
        | Action::GoToNextTab
        | Action::GoToPreviousTab
        | Action::ToggleTab
        | Action::MoveTab(..) => 20,
        Action::ScrollUp
        | Action::ScrollDown
        | Action::PageScrollUp
        | Action::PageScrollDown
        | Action::HalfPageScrollUp
        | Action::HalfPageScrollDown
        | Action::ScrollToBottom => 25,
        Action::ToggleFocusFullscreen
        | Action::ToggleFloatingPanes
        | Action::TogglePaneFrames
        | Action::TogglePaneEmbedOrFloating
        | Action::ToggleActiveSyncTab => 30,
        Action::NextSwapLayout | Action::PreviousSwapLayout => 32,
        Action::ToggleGroupMarking | Action::TogglePaneInGroup => 34,
        Action::PaneNameInput(..)
        | Action::TabNameInput(..)
        | Action::UndoRenamePane
        | Action::UndoRenameTab => 35,
        Action::Detach => 40,
        Action::SwitchToMode(mode) if *mode == base_mode => 60,
        Action::SwitchToMode(..) => 55,
        Action::Quit => 65,
        _ => 50,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(keys: &[&str], label: &str) -> Entry {
        Entry {
            priority: 10,
            keys: keys.iter().map(|s| s.to_string()).collect(),
            label: label.to_string(),
        }
    }

    #[test]
    fn display_width_counts_chars_not_bytes() {
        assert_eq!(display_width("abc"), 3);
        // Arrows are 3 bytes each but one column wide.
        assert_eq!(display_width("← ↓ ↑ →"), 7);
    }

    #[test]
    fn truncate_never_splits_a_multibyte_char() {
        // The bug that used to panic: byte-slicing through a multibyte char.
        let s = "→ Resize mode that is quite long";
        for max in 0..=display_width(s) + 2 {
            let out = truncate_to_width(s, max);
            assert!(out.chars().count() <= max.max(1));
            // Must always be valid UTF-8 / not panic, and round-trip as chars.
            assert_eq!(out, out.chars().collect::<String>());
        }
    }

    #[test]
    fn truncate_adds_ellipsis_when_too_long() {
        assert_eq!(truncate_to_width("abcdef", 4), "abc…");
        assert_eq!(truncate_to_width("abc", 4), "abc");
        assert_eq!(truncate_to_width("abc", 0), "");
    }

    #[test]
    fn pad_right_pads_to_width_and_never_truncates() {
        assert_eq!(pad_right("ab", 4), "ab  ");
        assert_eq!(pad_right("abcd", 2), "abcd");
        assert_eq!(display_width(&pad_right("←", 3)), 3);
    }

    #[test]
    fn layout_shows_everything_when_it_fits() {
        let entries = vec![entry(&["h"], "Focus Left"), entry(&["l"], "Focus Right")];
        let layout = compute_layout(&entries, 120, 40, 40);
        assert_eq!(layout.visible, 2);
        assert_eq!(layout.overflow, 0);
    }

    #[test]
    fn layout_overflows_when_too_many_for_the_height_cap() {
        let entries: Vec<Entry> = (0..50)
            .map(|i| entry(&["x"], &format!("Action {}", i)))
            .collect();
        let layout = compute_layout(&entries, 120, 30, 40);
        assert!(layout.visible >= 1);
        assert!(layout.overflow > 0);
        assert_eq!(layout.visible + layout.overflow, 50);
    }

    #[test]
    fn layout_width_respects_the_display_and_caps() {
        let entries = vec![entry(&["h"], "A short label")];
        let narrow = compute_layout(&entries, 20, 40, 40);
        assert!(narrow.pane_cols <= 20);
    }

    #[test]
    fn keys_string_joins_with_spaces() {
        assert_eq!(entry(&["h", "←"], "Focus Left").keys_str(), "h ←");
    }
}
