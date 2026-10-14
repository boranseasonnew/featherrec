use crate::preferences::{HotkeyBindings, Shortcut};
use anyhow::{Result, anyhow, bail};
use eframe::egui;
use std::{
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const WAKE: u32 = WM_APP + 17;
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Toggle,
    Stop,
    Mute,
}
struct Configure {
    bindings: HotkeyBindings,
    reply: SyncSender<Result<(), String>>,
}

fn register(id: i32, key: Shortcut) -> Result<(), String> {
    let mut flags = MOD_NOREPEAT;
    if key.ctrl {
        flags |= MOD_CONTROL;
    }
    if key.alt {
        flags |= MOD_ALT;
    }
    if key.shift {
        flags |= MOD_SHIFT;
    }
    unsafe { RegisterHotKey(None, id, flags, key.key) }
        .map_err(|error| format!("{} kullanılamıyor; başka bir uygulama kullanıyor olabilir. Farklı bir kısayol seç. ({error})", key.label()))
}
fn unregister_all() {
    for id in 1..=3 {
        let _ = unsafe { UnregisterHotKey(None, id) };
    }
}
fn register_all(bindings: &HotkeyBindings) -> Result<(), String> {
    if !bindings.enabled {
        return Ok(());
    }
    for (id, key) in [(1, bindings.toggle), (2, bindings.stop), (3, bindings.mute)] {
        if let Err(error) = register(id, key) {
            unregister_all();
            return Err(error);
        }
    }
    Ok(())
}

pub struct Hotkeys {
    id: u32,
    commands: SyncSender<Configure>,
    pub events: Receiver<Action>,
    worker: Option<JoinHandle<()>>,
}
impl Hotkeys {
    pub fn new(ctx: egui::Context) -> Result<Self> {
        let (commands, input) = mpsc::sync_channel::<Configure>(8);
        let (events, output) = mpsc::sync_channel(8);
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("featherrec-hotkeys".into()).spawn(move || {
            let mut message = MSG::default();
            // Ensure the thread has a message queue before publishing its ID.
            let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
            let id = unsafe { GetCurrentThreadId() };
            let _ = ready.send(id);
            let mut active = HotkeyBindings { enabled: false, ..HotkeyBindings::default() };
            loop {
                let received = unsafe { GetMessageW(&mut message, None, 0, 0) };
                if received.0 <= 0 { break; }
                if message.message == WAKE {
                    while let Ok(command) = input.try_recv() {
                        let result = command.bindings.validate().map_err(|e| e.to_string()).and_then(|()| {
                            unregister_all();
                            match register_all(&command.bindings) {
                                Ok(()) => { active = command.bindings; Ok(()) },
                                Err(error) => {
                                    if let Err(restore) = register_all(&active) { return Err(format!("{error} Önceki kısayollar geri yüklenemedi: {restore}")); }
                                    Err(error)
                                }
                            }
                        });
                        let _ = command.reply.send(result);
                        ctx.request_repaint();
                    }
                } else if message.message == WM_HOTKEY {
                    let action = match message.wParam.0 { 1 => Action::Toggle, 2 => Action::Stop, 3 => Action::Mute, _ => continue };
                    let _ = events.try_send(action);
                    ctx.request_repaint();
                }
            }
            unregister_all();
        })?;
        let id = started
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| anyhow!("Kısayol iş parçacığı başlatılamadı: {e}"))?;
        Ok(Self {
            id,
            commands,
            events: output,
            worker: Some(worker),
        })
    }
    pub fn configure(&self, bindings: HotkeyBindings) -> Result<Receiver<Result<(), String>>> {
        let (reply, result) = mpsc::sync_channel(1);
        self.commands
            .try_send(Configure { bindings, reply })
            .map_err(|_| anyhow!("Kısayol ayarı kuyruğu dolu"))?;
        unsafe { PostThreadMessageW(self.id, WAKE, WPARAM(0), LPARAM(0)) }?;
        Ok(result)
    }
    pub fn suspend(&self) -> Result<Receiver<Result<(), String>>> {
        self.configure(HotkeyBindings {
            enabled: false,
            ..HotkeyBindings::default()
        })
    }
    #[cfg(test)]
    fn inject_message(&self, action: usize) -> Result<()> {
        unsafe { PostThreadMessageW(self.id, WM_HOTKEY, WPARAM(action), LPARAM(0)) }?;
        Ok(())
    }
}
impl Drop for Hotkeys {
    fn drop(&mut self) {
        let _ = unsafe { PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn from_key(key: egui::Key, modifiers: egui::Modifiers) -> Result<Shortcut> {
    if modifiers.mac_cmd {
        bail!("Windows tuşu yerine Ctrl veya Alt kullan.");
    }
    let label = format!("{key:?}");
    let code = if label.len() == 1 {
        label.as_bytes()[0] as u32
    } else if let Some(number) = label.strip_prefix("Num") {
        number
            .parse::<u32>()
            .map_err(|_| anyhow!("Desteklenmeyen tuş"))?
            + 0x30
    } else if let Some(number) = label.strip_prefix('F') {
        number
            .parse::<u32>()
            .map_err(|_| anyhow!("Desteklenmeyen tuş"))?
            + 0x6F
    } else {
        bail!("Harf, rakam veya F1–F24 kullan.");
    };
    let shortcut = Shortcut {
        key: code,
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
    };
    shortcut.validate()?;
    Ok(shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotkey_thread_delivers_actions_and_stops() -> Result<()> {
        let service = Hotkeys::new(egui::Context::default())?;
        service.inject_message(1)?;
        assert!(matches!(
            service
                .events
                .recv_timeout(std::time::Duration::from_secs(2))?,
            Action::Toggle
        ));
        // Real OS key injection/registration conflicts are validated separately.
        Ok(())
    }
}
