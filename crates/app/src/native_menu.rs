//! The macOS menu bar: Pulse, Archivo, Edición, Crear, Ver, Transporte, Ventana and
//! Ayuda, with the usual ⌘ shortcuts. Items run the app's commands (see `commands`);
//! they are greyed out when they cannot act. Shortcuts without ⌘ (space, tab, M, F9)
//! stay with the app so they never steal keys from text fields. Elsewhere there is
//! no menu bar: the in-app menu has the same commands.

use crate::commands::Command;

/// One entry of a menu: a command with its title and shortcut, a separator, a submenu
/// of recent projects, or a system action (hide, minimize…).
#[derive(Clone, Copy)]
pub enum Item {
    Run(Command, &'static str, &'static str),
    Separator,
    Recent,
    // Only macOS's menu bar has them (the in-window menu leaves them out).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    System(&'static str, &'static str, &'static str),
}

use Command as C;
use Item::*;

/// The menus, in order. Shortcuts: a key, with "⇧" or "⌃" before it for Shift or
/// Control as well as ⌘.
pub const MENUS: [(&str, &[Item]); 8] = [
    (
        "Pulse",
        &[
            Run(C::About, "Acerca de Pulse", ""),
            Run(C::CheckUpdates, "Buscar actualizaciones…", ""),
            Separator,
            Run(C::Settings, "Ajustes…", ","),
            Separator,
            System("Ocultar Pulse", "hide:", "h"),
            System("Ocultar otras", "hideOtherApplications:", "⌥h"),
            System("Mostrar todas", "unhideAllApplications:", ""),
            Separator,
            Run(C::Quit, "Salir de Pulse", "q"),
        ],
    ),
    (
        "Archivo",
        &[
            Run(C::New, "Nuevo", "n"),
            Run(C::Open, "Abrir…", "o"),
            Recent,
            Separator,
            Run(C::Save, "Guardar", "s"),
            Run(C::SaveAs, "Guardar como…", "⇧s"),
            Run(C::Revert, "Volver a la versión guardada", ""),
            Run(C::Versions, "Versiones…", "⌥s"),
            Separator,
            Run(C::ImportLive, "Importar set de Live (.als)…", "i"),
            Run(C::ImportAudio, "Importar audio…", ""),
            Run(C::Export, "Exportar audio…", "⇧r"),
            Run(C::MixAssistant, "Asistente de mezcla y master…", ""),
            Separator,
            Run(C::ShowFolder, "Mostrar la carpeta de Pulse", ""),
        ],
    ),
    (
        "Edición",
        &[
            Run(C::Undo, "Deshacer", "z"),
            Run(C::Redo, "Rehacer", "⇧z"),
            Separator,
            Run(C::Cut, "Cortar", "x"),
            Run(C::Copy, "Copiar", "c"),
            Run(C::Paste, "Pegar", "v"),
            Run(C::Duplicate, "Duplicar", "d"),
            Run(C::Split, "Dividir clip", "e"),
            Run(C::Join, "Unir clips", "j"),
            Run(C::Delete, "Eliminar  (⌫)", ""),
            Separator,
            Run(C::SelectAll, "Seleccionar todas las notas", "a"),
        ],
    ),
    (
        "Crear",
        &[
            Run(C::NewMidiTrack, "Pista MIDI", "t"),
            Run(C::NewAudioTrack, "Pista de audio", "⇧t"),
            Separator,
            Run(C::DuplicateTrack, "Duplicar pista", ""),
            Run(C::DeleteTrack, "Eliminar pista", ""),
            Separator,
            Run(C::GroupTracks, "Agrupar pistas", "g"),
            Run(C::Ungroup, "Desagrupar", "⇧g"),
            Separator,
            Run(C::Arrange, "Arreglo automático…", ""),
        ],
    ),
    (
        "Ver",
        &[
            Run(C::ShowSession, "Session  (Tab)", ""),
            Run(C::ShowArrangement, "Arrangement  (Tab)", ""),
            Run(C::ToggleDetail, "Clip o dispositivos  (⇧Tab)", ""),
            Run(C::ToggleBrowser, "Mostrar u ocultar el navegador", ""),
            Separator,
            Run(C::FullScreen, "Pantalla completa", "⌃f"),
        ],
    ),
    (
        "Transporte",
        &[
            Run(C::PlayStop, "Reproducir / Parar  (Espacio)", ""),
            Run(C::Record, "Grabar en el Arrangement  (F9)", ""),
            Run(C::Loop, "Loop", ""),
            Run(C::Metronome, "Metrónomo", ""),
            Run(C::AutomationArm, "Armar automatización", ""),
            Separator,
            Run(C::ComputerKeyboard, "Teclado del ordenador  (M)", ""),
            Run(C::MidiAssign, "Asignar controles MIDI", ""),
        ],
    ),
    ("Ventana", &[System("Minimizar", "performMiniaturize:", "m"), System("Zoom", "performZoom:", "")]),
    ("Ayuda", &[Run(C::Guide, "Guía de Pulse", ""), Run(C::Shortcuts, "Atajos de teclado", "")]),
];

/// Where there is no system menu bar (Windows, Linux) the menus' shortcuts are read
/// from the keyboard here, with Ctrl for ⌘ and F11 for full screen. Copy, cut and
/// paste come to egui as events of their own. `handled` are read elsewhere (see
/// `handle_shortcuts`), and selecting every note goes to the clip editor as a key.
pub fn pressed(ctx: &eframe::egui::Context, handled: &[Command]) -> Vec<Command> {
    use eframe::egui::{Event, Key, KeyboardShortcut, Modifiers};
    let mut found = Vec::new();
    ctx.input_mut(|i| {
        for event in &i.events {
            match event {
                Event::Copy => found.push(C::Copy),
                Event::Cut => found.push(C::Cut),
                Event::Paste(_) => found.push(C::Paste),
                _ => {}
            }
        }
        if i.consume_key(Modifiers::NONE, Key::F11) {
            found.push(C::FullScreen);
        }
        for item in MENUS.iter().flat_map(|(_, items)| items.iter()) {
            let Run(command, _, shortcut) = *item else { continue };
            if shortcut.is_empty() || handled.contains(&command) || matches!(command, C::FullScreen | C::SelectAll | C::Copy | C::Cut | C::Paste) {
                continue;
            }
            let Some((modifiers, key)) = parse(shortcut) else { continue };
            if i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)) {
                found.push(command);
            }
        }
    });
    found
}

/// A menu shortcut ("⇧s", ",") as ⌘ (Ctrl off macOS) plus modifiers and a key.
fn parse(shortcut: &str) -> Option<(eframe::egui::Modifiers, eframe::egui::Key)> {
    let mut modifiers = eframe::egui::Modifiers::COMMAND;
    let mut key = None;
    for c in shortcut.chars() {
        match c {
            '⇧' => modifiers = modifiers | eframe::egui::Modifiers::SHIFT,
            '⌥' => modifiers = modifiers | eframe::egui::Modifiers::ALT,
            '⌃' => modifiers = modifiers | eframe::egui::Modifiers::CTRL,
            ',' => key = Some(eframe::egui::Key::Comma),
            c => key = eframe::egui::Key::from_name(&c.to_uppercase().to_string()),
        }
    }
    Some((modifiers, key?))
}

/// Every command in the menus, in order (a menu item's tag is its index here).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn commands() -> Vec<Command> {
    MENUS.iter().flat_map(|(_, items)| items.iter()).filter_map(|item| if let Run(c, ..) = item { Some(*c) } else { None }).collect()
}

/// The standard "About" panel (or a status line elsewhere).
pub fn about() {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
    }
}

#[cfg(target_os = "macos")]
pub use mac::{describe, MenuBar};

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::commands::MAX_RECENT;
    use objc2::rc::Retained;
    use objc2::runtime::Sel;
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem, NSMenuItemValidation};
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// Recent projects use tags from here on.
    const RECENT_TAG: isize = 10_000;
    const CLEAR_RECENT_TAG: isize = 20_000;

    pub struct TargetState {
        /// Tags of the items chosen since the last frame.
        chosen: Rc<RefCell<Vec<isize>>>,
        /// Tags that cannot act now.
        disabled: Rc<RefCell<Vec<isize>>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = TargetState]
        struct MenuTarget;

        impl MenuTarget {
            #[unsafe(method(menuAction:))]
            fn menu_action(&self, sender: &NSMenuItem) {
                self.ivars().chosen.borrow_mut().push(sender.tag());
            }
        }

        unsafe impl NSObjectProtocol for MenuTarget {}

        unsafe impl NSMenuItemValidation for MenuTarget {
            #[unsafe(method(validateMenuItem:))]
            fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
                !self.ivars().disabled.borrow().contains(&item.tag())
            }
        }
    );

    /// The menu bar, once installed.
    pub struct MenuBar {
        target: Retained<MenuTarget>,
        chosen: Rc<RefCell<Vec<isize>>>,
        disabled: Rc<RefCell<Vec<isize>>>,
        commands: Vec<Command>,
        recent_menu: Retained<NSMenu>,
        recent_shown: Vec<PathBuf>,
        mtm: MainThreadMarker,
    }

    fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, shortcut: &str) -> Retained<NSMenuItem> {
        let (mut mask, mut key) = (NSEventModifierFlags::Command, shortcut.to_string());
        for (symbol, flag) in [("⇧", NSEventModifierFlags::Shift), ("⌃", NSEventModifierFlags::Control), ("⌥", NSEventModifierFlags::Option)] {
            if let Some(rest) = key.strip_prefix(symbol) {
                mask |= flag;
                key = rest.to_string();
            }
        }
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), action, &NSString::from_str(&key)) };
        if !key.is_empty() {
            item.setKeyEquivalentModifierMask(mask);
        }
        item
    }

    impl MenuBar {
        /// Builds the menus and makes them the application's. Main thread only.
        pub fn install() -> Option<MenuBar> {
            let mtm = MainThreadMarker::new()?;
            let chosen = Rc::new(RefCell::new(Vec::new()));
            let disabled = Rc::new(RefCell::new(Vec::new()));
            let target = MenuTarget::alloc(mtm).set_ivars(TargetState { chosen: chosen.clone(), disabled: disabled.clone() });
            let target: Retained<MenuTarget> = unsafe { msg_send![super(target), init] };
            let bar = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(""));
            let recent_menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("Abrir recientes"));
            let commands = super::commands();
            let mut tag = 0;
            let app = NSApplication::sharedApplication(mtm);
            for (title, items) in MENUS {
                let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(title));
                for entry in items {
                    match *entry {
                        Run(_, title, shortcut) => {
                            let item = item(mtm, title, Some(sel!(menuAction:)), shortcut);
                            item.setTag(tag);
                            unsafe { item.setTarget(Some(&target)) };
                            menu.addItem(&item);
                            tag += 1;
                        }
                        Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                        Recent => {
                            let item = item(mtm, "Abrir recientes", None, "");
                            item.setSubmenu(Some(&recent_menu));
                            menu.addItem(&item);
                        }
                        System(title, action, shortcut) => {
                            let item = item(mtm, title, Some(Sel::register(&std::ffi::CString::new(action).unwrap())), shortcut);
                            menu.addItem(&item);
                        }
                    }
                }
                let holder = item(mtm, title, None, "");
                holder.setSubmenu(Some(&menu));
                bar.addItem(&holder);
                if title == "Ventana" {
                    app.setWindowsMenu(Some(&menu));
                }
            }
            app.setMainMenu(Some(&bar));
            let mut menu_bar = MenuBar { target, chosen, disabled, commands, recent_menu, recent_shown: vec![PathBuf::from("-")], mtm };
            menu_bar.set_recent(&[]);
            Some(menu_bar)
        }

        /// The commands chosen since the last call.
        pub fn chosen(&self) -> Vec<Command> {
            let tags: Vec<isize> = self.chosen.borrow_mut().drain(..).collect();
            tags.into_iter()
                .filter_map(|tag| match tag {
                    CLEAR_RECENT_TAG => Some(Command::ClearRecent),
                    t if t >= RECENT_TAG => Some(Command::OpenRecent((t - RECENT_TAG) as usize)),
                    t => self.commands.get(t as usize).copied(),
                })
                .collect()
        }

        /// Greys out what cannot act, and keeps the recent projects up to date.
        pub fn update(&mut self, enabled: impl Fn(Command) -> bool, recent: &[PathBuf]) {
            let mut disabled: Vec<isize> = self.commands.iter().enumerate().filter(|(_, c)| !enabled(**c)).map(|(i, _)| i as isize).collect();
            if recent.is_empty() {
                disabled.push(CLEAR_RECENT_TAG);
            }
            *self.disabled.borrow_mut() = disabled;
            self.set_recent(recent);
        }

        fn set_recent(&mut self, recent: &[PathBuf]) {
            if self.recent_shown == recent {
                return;
            }
            self.recent_shown = recent.to_vec();
            let menu = &self.recent_menu;
            menu.removeAllItems();
            for (i, path) in recent.iter().take(MAX_RECENT).enumerate() {
                let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
                let item = item(self.mtm, &name, Some(sel!(menuAction:)), "");
                item.setTag(RECENT_TAG + i as isize);
                unsafe { item.setTarget(Some(&self.target)) };
                item.setToolTip(Some(&NSString::from_str(&path.display().to_string())));
                menu.addItem(&item);
            }
            if recent.is_empty() {
                let none = item(self.mtm, "Ningún proyecto reciente", None, "");
                menu.addItem(&none);
            }
            menu.addItem(&NSMenuItem::separatorItem(self.mtm));
            let clear = item(self.mtm, "Borrar la lista", Some(sel!(menuAction:)), "");
            clear.setTag(CLEAR_RECENT_TAG);
            unsafe { clear.setTarget(Some(&self.target)) };
            menu.addItem(&clear);
        }
    }

    /// The installed menu bar as text (title, shortcut, enabled), checking it without
    /// showing anything: `pulse --check-menus`.
    pub fn describe(enabled: impl Fn(Command) -> bool) -> Vec<String> {
        let Some(mtm) = MainThreadMarker::new() else { return vec!["no es el hilo principal".into()] };
        let app = NSApplication::sharedApplication(mtm);
        // An application object that never shows up in the Dock.
        let _: bool = unsafe { msg_send![&*app, setActivationPolicy: 2isize] };
        let Some(mut bar) = MenuBar::install() else { return vec!["no se pudo instalar".into()] };
        bar.update(&enabled, &[PathBuf::from("/tmp/Demo.pulse")]);
        let mut lines = Vec::new();
        let Some(main) = app.mainMenu() else { return vec!["sin menú principal".into()] };
        for i in 0..main.numberOfItems() {
            let Some(holder) = main.itemAtIndex(i) else { continue };
            let Some(menu) = holder.submenu() else { continue };
            lines.push(format!("[{}]", menu.title()));
            for k in 0..menu.numberOfItems() {
                let Some(item) = menu.itemAtIndex(k) else { continue };
                if item.isSeparatorItem() {
                    continue;
                }
                let key = item.keyEquivalent().to_string();
                let mask = item.keyEquivalentModifierMask();
                let mut shortcut = String::new();
                if !key.is_empty() {
                    for (flag, symbol) in [(NSEventModifierFlags::Control, "⌃"), (NSEventModifierFlags::Option, "⌥"), (NSEventModifierFlags::Shift, "⇧"), (NSEventModifierFlags::Command, "⌘")] {
                        if mask.contains(flag) {
                            shortcut.push_str(symbol);
                        }
                    }
                    shortcut.push_str(&key.to_uppercase());
                }
                let ours = item.target().is_some();
                let active = item.action().is_some() && (!ours || !bar.disabled.borrow().contains(&item.tag()));
                let sub = item.submenu().map(|m| format!(" ▸ {} elementos", m.numberOfItems())).unwrap_or_default();
                lines.push(format!("  {}{}{}{}", item.title(), if shortcut.is_empty() { String::new() } else { format!("  {shortcut}") }, if active || item.submenu().is_some() { "" } else { "  (desactivado)" }, sub));
            }
        }
        lines
    }
}

/// Elsewhere: no menu bar.
#[cfg(not(target_os = "macos"))]
pub struct MenuBar;

#[cfg(not(target_os = "macos"))]
impl MenuBar {
    pub fn install() -> Option<MenuBar> {
        None
    }

    pub fn chosen(&self) -> Vec<Command> {
        Vec::new()
    }

    pub fn update(&mut self, _enabled: impl Fn(Command) -> bool, _recent: &[std::path::PathBuf]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_shortcuts_are_read_where_there_is_no_menu_bar() {
        let ctx = eframe::egui::Context::default();
        let press = |key: eframe::egui::Key, modifiers: eframe::egui::Modifiers| {
            let mut input = eframe::egui::RawInput::default();
            input.events.push(eframe::egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers });
            let mut found = Vec::new();
            let _ = ctx.run_ui(input, |ui| found = pressed(ui.ctx(), &[C::Save, C::Undo]));
            found
        };
        assert_eq!(press(eframe::egui::Key::E, eframe::egui::Modifiers::COMMAND), vec![C::Split]);
        assert_eq!(press(eframe::egui::Key::J, eframe::egui::Modifiers::COMMAND), vec![C::Join]);
        assert_eq!(press(eframe::egui::Key::Comma, eframe::egui::Modifiers::COMMAND), vec![C::Settings]);
        assert_eq!(press(eframe::egui::Key::F11, eframe::egui::Modifiers::NONE), vec![C::FullScreen]);
        // Read elsewhere, or without Ctrl: nothing here.
        assert!(press(eframe::egui::Key::S, eframe::egui::Modifiers::COMMAND).is_empty());
        assert!(press(eframe::egui::Key::E, eframe::egui::Modifiers::NONE).is_empty());
        let mut input = eframe::egui::RawInput::default();
        input.events.push(eframe::egui::Event::Copy);
        let mut found = Vec::new();
        let _ = ctx.run_ui(input, |ui| found = pressed(ui.ctx(), &[]));
        assert_eq!(found, vec![C::Copy]);
    }

    #[test]
    fn every_command_with_a_shortcut_is_in_the_menus_once() {
        let commands = commands();
        let mut unique = commands.clone();
        unique.dedup();
        assert_eq!(unique.len(), commands.len());
        for c in [C::New, C::Open, C::Save, C::SaveAs, C::Revert, C::Quit, C::Undo, C::Redo, C::Copy, C::Paste, C::Export, C::Shortcuts] {
            assert!(commands.contains(&c), "{c:?}");
        }
        // ⌘ shortcuts are unique.
        let mut keys: Vec<&str> = MENUS.iter().flat_map(|(_, items)| items.iter()).filter_map(|i| match i {
            Run(_, _, k) | System(_, _, k) if !k.is_empty() => Some(*k),
            _ => None,
        }).collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "no shortcut twice");
    }
}
