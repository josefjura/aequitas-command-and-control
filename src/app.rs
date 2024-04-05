use crate::{
    action::Action,
    components::Component,
    screen::{Mode, Screen},
    tui,
};
use color_eyre::eyre;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::prelude::Rect;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    Success,
    Error,
    Info,
}

pub struct App {
    pub current_screen: Mode,
    pub exit: bool,
    pub suspend: bool,
    pub tick_rate: f64,
    pub frame_rate: f64,
    pub screens: Vec<Screen>,
    pub config: HashMap<String, String>,
}

impl App {
    pub fn new(screens: Vec<Screen>, config: HashMap<String, String>) -> Self {
        Self {
            current_screen: Mode::FileChooser,
            exit: false,
            suspend: false,
            frame_rate: 30.0,
            tick_rate: 1.0,
            screens,
            config,
        }
    }

    pub async fn run(&mut self) -> eyre::Result<()> {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();

        let mut tui = tui::Tui::new()?
            .tick_rate(self.tick_rate)
            .frame_rate(self.frame_rate);
        // tui.mouse(true);
        tui.enter()?;

        for screen in self.screens.iter_mut() {
            for component in screen.components.iter_mut() {
                component.register_action_handler(action_tx.clone())?;
            }
        }

        for screen in self.screens.iter_mut() {
            for component in screen.components.iter_mut() {
                component.register_config_handler(self.config.clone())?;
            }
        }

        for screen in self.screens.iter_mut() {
            for component in screen.components.iter_mut() {
                component.init(tui.size()?)?;
            }
        }

        loop {
            if let Some(e) = tui.next().await {
                match e {
                    tui::Event::Quit => action_tx.send(Action::Quit)?,
                    tui::Event::Tick => action_tx.send(Action::Tick)?,
                    tui::Event::Render => action_tx.send(Action::Render)?,
                    tui::Event::Resize(x, y) => action_tx.send(Action::Resize(x, y))?,
                    tui::Event::SwitchMode(mode) => action_tx.send(Action::SwitchMode(mode))?,
                    tui::Event::Key(key) => match (self.current_screen, key.code) {
                        (_, KeyCode::Char('z')) if key.modifiers == KeyModifiers::CONTROL => {
                            action_tx.send(Action::Suspend)?
                        }
                        (_, KeyCode::Char('c')) if key.modifiers == KeyModifiers::CONTROL => {
                            action_tx.send(Action::Quit)?
                        }
                        (_, KeyCode::Char('q')) => action_tx.send(Action::Quit)?,
                        (_, KeyCode::Char('r')) => action_tx.send(Action::ScriptRun)?,
                        (_, KeyCode::Char(' ')) => action_tx.send(Action::SelectCurrent)?,
                        (_, KeyCode::Char('s')) => action_tx.send(Action::SelectAllAfter)?,
                        (_, KeyCode::Char('S')) => action_tx.send(Action::SelectAllInDirectory)?,
                        (_, KeyCode::Char('X')) => {
                            action_tx.send(Action::RemoveAllSelectedScripts)?
                        }
                        (_, KeyCode::Char('x')) => action_tx.send(Action::RemoveSelectedScript)?,
                        (_, KeyCode::Up) => action_tx.send(Action::CursorUp)?,
                        (_, KeyCode::Down) => action_tx.send(Action::CursorDown)?,
                        (_, KeyCode::Home) => action_tx.send(Action::CursorToTop)?,
                        (_, KeyCode::End) => action_tx.send(Action::CursorToBottom)?,
                        (_, KeyCode::Enter) => action_tx.send(Action::DirectoryOpenSelected)?,
                        (_, KeyCode::Backspace) => action_tx.send(Action::DirectoryLeave)?,
                        (Mode::FileChooser, KeyCode::Tab) => {
                            action_tx.send(Action::SwitchMode(Mode::ScriptRunner))?
                        }
                        (Mode::ScriptRunner, KeyCode::Tab) => {
                            action_tx.send(Action::SwitchMode(Mode::FileChooser))?
                        }
                        _ => {}
                    },
                    _ => {}
                }

                for screen in self.screens.iter_mut() {
                    for component in screen.components.iter_mut() {
                        if let Some(action) = component.handle_events(Some(e.clone()))? {
                            action_tx.send(action)?;
                        }
                    }
                }
            }

            while let Ok(action) = action_rx.try_recv() {
                if action != Action::Tick && action != Action::Render {
                    log::debug!("{action:?}");
                }
                match action {
                    Action::Tick => {
                        //self.last_tick_key_events.drain(..);
                    }
                    Action::Quit => self.exit = true,
                    Action::Suspend => self.suspend = true,
                    Action::Resume => self.suspend = false,
                    Action::SwitchMode(mode) => self.current_screen = mode,
                    Action::Resize(w, h) => {
                        tui.resize(Rect::new(0, 0, w, h))?;
                        let screen = self
                            .screens
                            .iter_mut()
                            .find(|f| f.mode == self.current_screen);
                        if let Some(screen) = screen {
                            tui.draw(|f| {
                                for component in screen.components.iter_mut() {
                                    let r = component.draw(f, f.size());
                                    if let Err(e) = r {
                                        action_tx
                                            .send(Action::Error(format!("Failed to draw: {:?}", e)))
                                            .unwrap();
                                    }
                                }
                            })?;
                        }
                    }
                    Action::Render => {
                        let screen = self
                            .screens
                            .iter_mut()
                            .find(|f| f.mode == self.current_screen);
                        if let Some(screen) = screen {
                            tui.draw(|f| {
                                for component in screen.components.iter_mut() {
                                    let r = component.draw(f, f.size());
                                    if let Err(e) = r {
                                        action_tx
                                            .send(Action::Error(format!("Failed to draw: {:?}", e)))
                                            .unwrap();
                                    }
                                }
                            })?;
                        }
                    }
                    _ => {}
                }
                let screen = self
                    .screens
                    .iter_mut()
                    .find(|f| f.mode == self.current_screen);
                if let Some(screen) = screen {
                    for component in screen.components.iter_mut() {
                        if let Some(action) = component.update(action.clone())? {
                            action_tx.send(action)?
                        };
                    }
                }

                for screen in self.screens.iter_mut() {
                    for component in screen.components.iter_mut() {
                        if let Some(action) = component.update_background(action.clone()).await? {
                            action_tx.send(action)?
                        };
                    }
                }
            }
            if self.suspend {
                tui.suspend()?;
                action_tx.send(Action::Resume)?;
                tui = tui::Tui::new()?
                    .tick_rate(self.tick_rate)
                    .frame_rate(self.frame_rate);
                // tui.mouse(true);
                tui.enter()?;
            } else if self.exit {
                tui.stop()?;
                break;
            }
        }
        tui.exit()?;
        Ok(())
    }
}
