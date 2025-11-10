use ansi_term::{Colour, Style};
use std::collections::BTreeMap;
use zellij_tile::prelude::actions::Action;
use zellij_tile::prelude::*;

#[derive(Default)]
struct State {
    mode_info: ModeInfo,
    auto_show: bool,
    hide_in_base_mode: bool,
    max_lines: usize,
    permissions_granted: bool,
    own_plugin_id: Option<u32>,
    rows: usize,
    cols: usize,
    display_area_rows: usize,
    display_area_cols: usize,
    // New fields for dual-mode operation
    is_overlay: bool,      // True if this is the overlay instance
    overlay_visible: bool, // For main instance: track if overlay exists
}

impl State {
    // Main instance: spawn the overlay as a new floating plugin instance
    fn spawn_overlay(&mut self) {
        if self.overlay_visible {
            return; // Already visible
        }

        eprintln!("zj-which-key: Spawning overlay instance");

        let coordinates = self.calculate_coordinates();
        let mut config = BTreeMap::new();
        config.insert("is_overlay".to_string(), "true".to_string());
        config.insert("max_lines".to_string(), self.max_lines.to_string());
        config.insert(
            "hide_in_base_mode".to_string(),
            self.hide_in_base_mode.to_string(),
        );

        let message = MessageToPlugin::new("spawn_overlay")
            .with_plugin_url("zellij:OWN_URL")
            .with_plugin_config(config)
            .with_floating_pane_coordinates(coordinates)
            .new_plugin_instance_should_have_pane_title(format!("{:?} Mode", self.mode_info.mode));

        pipe_message_to_plugin(message);
        self.overlay_visible = true;
    }

    // Main instance: mark overlay as closed
    // (The overlay instance closes itself autonomously)
    fn close_overlay(&mut self) {
        if !self.overlay_visible {
            return;
        }

        eprintln!("zj-which-key: Marking overlay as closed (it closes itself)");
        self.overlay_visible = false;
        // Note: The overlay instance closes itself via close_self() when detecting base mode
    }

    // Main instance: toggle overlay visibility
    fn toggle_overlay(&mut self) {
        eprintln!(
            "zj-which-key: Toggle called, overlay_visible={}",
            self.overlay_visible
        );
        if self.overlay_visible {
            self.close_overlay();
        } else {
            self.spawn_overlay();
        }
    }

    fn calculate_coordinates(&self) -> FloatingPaneCoordinates {
        let left_margin = 2;
        let right_margin = 2;

        let terminal_cols = if self.display_area_cols > 0 {
            self.display_area_cols
        } else {
            255
        };

        let terminal_rows = if self.display_area_rows > 0 {
            self.display_area_rows
        } else {
            70
        };

        let width = terminal_cols.saturating_sub(left_margin + right_margin);
        let height = 14; // Half the previous height for more compact display
        let x_position = left_margin;
        let y_position = terminal_rows.saturating_sub(height);

        eprintln!(
            "zj-which-key: Full-width coords x={}, y={}, width={} (terminal: {}x{})",
            x_position, y_position, width, terminal_cols, terminal_rows
        );

        FloatingPaneCoordinates::new(
            Some(x_position.to_string()),
            Some(y_position.to_string()),
            Some(width.to_string()),
            Some(height.to_string()),
            Some(true),
        )
        .unwrap_or_default()
    }

    fn is_base_mode(&self) -> bool {
        match self.mode_info.base_mode {
            Some(base) => self.mode_info.mode == base,
            None => self.mode_info.mode == InputMode::Normal,
        }
    }

    fn parse_config(&mut self, config: BTreeMap<String, String>) {
        // Check if this is the overlay instance
        self.is_overlay = config
            .get("is_overlay")
            .map(|s| s == "true")
            .unwrap_or(false);

        // Parse auto_show_on_mode_change (main instance only)
        self.auto_show = config
            .get("auto_show_on_mode_change")
            .map(|s| s == "true")
            .unwrap_or(true);

        // Parse hide_in_base_mode (main instance only)
        self.hide_in_base_mode = config
            .get("hide_in_base_mode")
            .map(|s| s == "true")
            .unwrap_or(true);

        // Parse max_lines
        self.max_lines = config
            .get("max_lines")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);

        eprintln!(
            "zj-which-key: Parsed config - is_overlay={}, auto_show={}, max_lines={}",
            self.is_overlay, self.auto_show, self.max_lines
        );
    }

    fn format_key(&self, key: &KeyWithModifier) -> String {
        let mut result = String::new();

        // Add modifiers
        if key.has_modifiers(&[KeyModifier::Ctrl]) {
            result.push_str("Ctrl+");
        }
        if key.has_modifiers(&[KeyModifier::Alt]) {
            result.push_str("Alt+");
        }
        if key.has_modifiers(&[KeyModifier::Shift]) {
            result.push_str("Shift+");
        }

        // Add the bare key
        match key.bare_key {
            BareKey::Char(c) => result.push(c),
            BareKey::F(n) => result.push_str(&format!("F{}", n)),
            BareKey::Enter => result.push_str("Enter"),
            BareKey::Esc => result.push_str("Esc"),
            BareKey::Tab => result.push_str("Tab"),
            BareKey::Backspace => result.push_str("Backspace"),
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

    fn format_action(&self, actions: &[Action]) -> String {
        if actions.is_empty() {
            return "Unknown".to_string();
        }

        // Take the first action as the primary one
        match &actions[0] {
            Action::Quit => "Quit Zellij".to_string(),
            Action::SwitchToMode(mode) => {
                // Check if switching to base mode
                let base_mode = self.mode_info.base_mode.unwrap_or(InputMode::Normal);
                if *mode == base_mode {
                    "← Back to Normal".to_string()
                } else {
                    format!("→ {:?} mode", mode)
                }
            }
            Action::Resize(resize, ..) => {
                format!("Resize {:?}", resize)
            }
            Action::MoveFocus(direction) => format!("Focus {:?}", direction),
            Action::NewPane(direction, ..) => match direction {
                Some(d) => format!("New pane {:?}", d),
                None => "New pane".to_string(),
            },
            Action::CloseFocus => "Close pane".to_string(),
            Action::NewTab(..) => "New tab".to_string(),
            Action::GoToTab(n) => format!("Tab {}", n),
            Action::GoToNextTab => "Next tab".to_string(),
            Action::GoToPreviousTab => "Prev tab".to_string(),
            Action::CloseTab => "Close tab".to_string(),
            Action::ToggleFloatingPanes => "Toggle floating".to_string(),
            Action::TogglePaneFrames => "Toggle frames".to_string(),
            Action::ToggleFocusFullscreen => "Fullscreen".to_string(),
            Action::PaneNameInput(..) => "Rename pane".to_string(),
            Action::TabNameInput(..) => "Rename tab".to_string(),
            Action::UndoRenamePane => "Undo rename".to_string(),
            Action::UndoRenameTab => "Undo rename".to_string(),
            Action::Run(..) => "Run".to_string(),
            Action::Write(_, bytes, _) => {
                if bytes.len() == 1 && bytes[0] == 10 {
                    "Enter".to_string()
                } else {
                    "Write".to_string()
                }
            }
            _ => format!("{:?}", actions[0]),
        }
    }

    fn is_mode_specific_action(&self, action: &Action) -> bool {
        match self.mode_info.mode {
            InputMode::Pane => matches!(
                action,
                Action::NewPane(..)
                    | Action::CloseFocus
                    | Action::MoveFocus(..)
                    | Action::Resize(..)
                    | Action::ToggleFocusFullscreen
                    | Action::ToggleFloatingPanes
                    | Action::TogglePaneFrames
                    | Action::PaneNameInput(..)
            ),
            InputMode::Tab => matches!(
                action,
                Action::NewTab(..)
                    | Action::CloseTab
                    | Action::GoToTab(..)
                    | Action::GoToNextTab
                    | Action::GoToPreviousTab
                    | Action::TabNameInput(..)
                    | Action::UndoRenameTab
            ),
            InputMode::Resize => matches!(action, Action::Resize(..)),
            InputMode::Move => matches!(action, Action::MoveFocus(..)),
            InputMode::Scroll => true,  // Show all for scroll mode
            InputMode::Session => true, // Show all for session mode
            _ => false,
        }
    }

    fn is_shared_action(&self, action: &Action) -> bool {
        let base_mode = self.mode_info.base_mode.unwrap_or(InputMode::Normal);
        match action {
            Action::SwitchToMode(mode) if *mode == base_mode => true,
            Action::Quit => true,
            _ => false,
        }
    }

    fn should_show_action(&self, actions: &[Action]) -> bool {
        if actions.is_empty() {
            return false;
        }

        let action = &actions[0];

        // Show mode-specific actions OR important shared actions
        self.is_mode_specific_action(action) || self.is_shared_action(action)
    }

    fn action_priority(&self, actions: &[Action]) -> u8 {
        if actions.is_empty() {
            return 255;
        }

        let action = &actions[0];
        let base_mode = self.mode_info.base_mode.unwrap_or(InputMode::Normal);

        // Mode-specific actions FIRST (highest priority)
        if self.is_mode_specific_action(action) {
            return match action {
                // Primary actions first
                Action::NewPane(..) | Action::NewTab(..) => 10,
                Action::CloseFocus | Action::CloseTab => 11,
                Action::MoveFocus(..) => 12,
                Action::Resize(..) => 13,

                // Navigation
                Action::GoToNextTab | Action::GoToPreviousTab | Action::GoToTab(..) => 20,

                // Toggles
                Action::ToggleFocusFullscreen
                | Action::ToggleFloatingPanes
                | Action::TogglePaneFrames => 30,

                // Naming
                Action::PaneNameInput(..) | Action::TabNameInput(..) => 35,

                _ => 15,
            };
        }

        // Shared actions BELOW mode-specific (lower priority = higher number)
        if matches!(action, Action::SwitchToMode(mode) if *mode == base_mode) {
            return 50; // Return to Normal
        }
        if matches!(action, Action::Quit) {
            return 51;
        }

        // Everything else filtered out
        100
    }

    fn priority_category(&self, priority: u8) -> Option<&str> {
        match priority {
            10..=19 => {
                // Mode-specific category based on current mode (shown FIRST)
                match self.mode_info.mode {
                    InputMode::Pane => Some("PANE ACTIONS"),
                    InputMode::Tab => Some("TAB ACTIONS"),
                    InputMode::Resize => Some("RESIZE ACTIONS"),
                    InputMode::Move => Some("MOVE ACTIONS"),
                    _ => Some("ACTIONS"),
                }
            }
            20..=29 => Some("NAVIGATION"),
            30..=39 => Some("TOGGLES"),
            50..=59 => Some("SHARED"), // Shown BELOW mode-specific
            _ => None,
        }
    }

    fn get_keybindings(&self) -> Vec<(u8, String, String)> {
        let keybinds = self.mode_info.get_mode_keybinds();

        eprintln!(
            "zj-which-key: Total keybindings for {:?} mode: {}",
            self.mode_info.mode,
            keybinds.len()
        );

        let mut bindings: Vec<_> = keybinds
            .iter()
            .map(|(key, actions)| {
                let key_str = self.format_key(key);
                let action_str = self.format_action(actions);
                let priority = self.action_priority(actions);
                let should_show = self.should_show_action(actions);
                eprintln!(
                    "  {} → {} (priority={}, show={})",
                    key_str, action_str, priority, should_show
                );
                (priority, key_str, action_str, should_show)
            })
            .filter(|(_, _, _, should_show)| *should_show)
            .map(|(p, k, a, _)| (p, k, a))
            .collect();

        eprintln!(
            "zj-which-key: After filtering: {} keybindings",
            bindings.len()
        );

        // Sort by priority, then by key string
        bindings.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        // Deduplicate by action (keep first occurrence)
        let mut seen_actions = std::collections::HashSet::new();
        bindings.retain(|(_, _, action)| seen_actions.insert(action.clone()));

        bindings
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Parse configuration first to know if we're overlay or main
        self.parse_config(configuration);

        // Get our plugin ID
        let plugin_ids = get_plugin_ids();
        self.own_plugin_id = Some(plugin_ids.plugin_id);

        eprintln!(
            "zj-which-key: Loading - is_overlay={}, plugin_id={}",
            self.is_overlay, plugin_ids.plugin_id
        );

        if self.is_overlay {
            // Overlay instance: minimal setup
            request_permission(&[PermissionType::ReadApplicationState]);
            subscribe(&[
                EventType::ModeUpdate,
                EventType::Key,
                EventType::CustomMessage,
            ]);
        } else {
            // Main instance: full setup including permission to spawn overlays
            request_permission(&[
                PermissionType::ReadApplicationState,
                PermissionType::ChangeApplicationState,
                PermissionType::MessageAndLaunchOtherPlugins, // CRITICAL: needed for pipe_message_to_plugin
            ]);
            subscribe(&[
                EventType::ModeUpdate,
                EventType::TabUpdate,
                EventType::Key,
                EventType::PermissionRequestResult,
                EventType::CustomMessage,
            ]);
        }

        self.permissions_granted = false;
        self.overlay_visible = false;
    }

    fn update(&mut self, event: Event) -> bool {
        if self.is_overlay {
            // Overlay instance: handle events for the floating overlay
            match event {
                Event::PermissionRequestResult(PermissionStatus::Granted) => {
                    self.permissions_granted = true;
                    set_selectable(false); // Can't be focused
                    false
                }

                Event::ModeUpdate(mode_info) => {
                    let was_base_mode = self.is_base_mode();
                    self.mode_info = mode_info;
                    let is_base_mode = self.is_base_mode();

                    // Overlay closes itself when returning to base mode
                    if self.hide_in_base_mode && !was_base_mode && is_base_mode {
                        eprintln!("zj-which-key: Overlay auto-closing (returned to base mode)");
                        close_self();
                        return false;
                    }

                    true // Re-render to show new mode
                }

                Event::Key(key) => {
                    // Ctrl+g closes the overlay
                    if matches!(key.bare_key, BareKey::Char('g'))
                        && key.has_modifiers(&[KeyModifier::Ctrl])
                    {
                        eprintln!("zj-which-key: Overlay closing via Ctrl+g");
                        close_self();
                    }
                    false
                }

                _ => false,
            }
        } else {
            // Main instance: handle spawning/closing overlay
            match event {
                Event::PermissionRequestResult(PermissionStatus::Granted) => {
                    eprintln!("zj-which-key: Main instance permissions granted");
                    self.permissions_granted = true;
                    false
                }

                Event::ModeUpdate(mode_info) => {
                    let was_base_mode = self.is_base_mode();
                    self.mode_info = mode_info;
                    let is_base_mode = self.is_base_mode();

                    // Auto-show/hide logic
                    if self.permissions_granted && self.auto_show {
                        if was_base_mode && !is_base_mode {
                            eprintln!("zj-which-key: Auto-spawning overlay (left base mode)");
                            self.spawn_overlay();
                        } else if !was_base_mode && is_base_mode && self.hide_in_base_mode {
                            eprintln!("zj-which-key: Auto-closing overlay (returned to base mode)");
                            self.close_overlay();
                        }
                    }

                    false // Main instance doesn't render
                }

                Event::TabUpdate(tabs) => {
                    // Update display area dimensions from active tab
                    for tab in tabs {
                        if tab.active {
                            self.display_area_rows = tab.display_area_rows;
                            self.display_area_cols = tab.display_area_columns;
                            eprintln!(
                                "zj-which-key: Terminal dimensions: {}x{}",
                                self.display_area_cols, self.display_area_rows
                            );
                            break;
                        }
                    }
                    false
                }

                Event::Key(key) => {
                    // Ctrl+g toggles overlay
                    if self.permissions_granted
                        && matches!(key.bare_key, BareKey::Char('g'))
                        && key.has_modifiers(&[KeyModifier::Ctrl])
                    {
                        eprintln!("zj-which-key: Ctrl+g detected - toggling overlay");
                        self.toggle_overlay();
                    }
                    false
                }

                _ => false,
            }
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        // Update dimensions
        self.rows = rows;
        self.cols = cols;

        // Only overlay instance renders
        if !self.is_overlay {
            return;
        }

        // Simple, clean styling
        let fg = Colour::Fixed(252);
        let key_bg = Colour::Fixed(240);
        let key_fg = Colour::Fixed(252);
        let cat_color = Colour::Fixed(245);

        // Get keybindings
        let keybindings = self.get_keybindings();

        // Calculate column layout
        let column_width = 30; // Width per keybinding column (badge + action + padding)
        let num_columns = (cols / column_width).max(1);

        // Header
        println!(
            "{} Mode",
            Style::new()
                .bold()
                .fg(fg)
                .paint(format!("{:?}", self.mode_info.mode))
        );
        println!();

        let mut kb_index = 0;
        let mut last_category: Option<&str> = None;

        while kb_index < keybindings.len() {
            // Check if current item starts a new category
            let (priority, _, _) = &keybindings[kb_index];
            let category = self.priority_category(*priority);

            if category != last_category && category.is_some() {
                if kb_index > 0 {
                    println!(); // Blank line before category
                }
                println!("{}", Style::new().fg(cat_color).paint(category.unwrap()));
                last_category = category;
            }

            // Render one row with multiple columns - but ONLY items from same category
            for col in 0..num_columns {
                if kb_index >= keybindings.len() {
                    break;
                }

                // Check if we're crossing into a different category
                let (item_priority, _, _) = &keybindings[kb_index];
                let item_category = self.priority_category(*item_priority);
                if item_category != last_category {
                    // Don't render this item yet - it's a new category
                    // Break out of column loop and let next iteration show category header
                    break;
                }

                let (_, key, action) = &keybindings[kb_index];

                // Highlight key badge
                let key_badge = format!(" {} ", key);
                print!(
                    "{}",
                    Style::new().fg(key_fg).on(key_bg).bold().paint(&key_badge)
                );
                print!(" ");

                // Color for action
                let action_color = if action.starts_with("← Back") {
                    Colour::Fixed(114)
                } else if action.starts_with("→") {
                    Colour::Fixed(180)
                } else {
                    fg
                };

                // Truncate action to fit column
                let max_action_len = column_width - key_badge.len() - 2;
                let action_display = if action.len() > max_action_len {
                    format!("{}…", &action[..max_action_len - 1])
                } else {
                    action.to_string()
                };

                print!(
                    "{:<width$}",
                    Style::new().fg(action_color).paint(&action_display),
                    width = max_action_len
                );

                kb_index += 1;

                // Add spacing between columns (except last)
                if col < num_columns - 1 && kb_index < keybindings.len() {
                    print!("  ");
                }
            }
            println!();
        }

        // Footer
        println!();
        print!("{}", Style::new().fg(fg).paint("Press "));
        print!(
            "{}",
            Style::new().fg(key_fg).on(key_bg).bold().paint(" Ctrl+g ")
        );
        println!("{}", Style::new().fg(fg).paint(" to hide"));
    }
}
