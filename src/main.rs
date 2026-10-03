mod app;
mod game;
mod map;
mod rng;
mod storage;
mod theme;
mod ui;

use std::io::stdout;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind,
};
use ratatui::crossterm::execute;

use app::App;

fn main() -> std::io::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("mahjongg {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut app = App::new();
    install_panic_hook();
    let mut terminal = ratatui::init();
    let result = execute!(stdout(), EnableMouseCapture).and_then(|()| run(&mut terminal, &mut app));

    app.shutdown();
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    if let Some(err) = &app.storage_error {
        eprintln!("mahjongg: could not save data: {err}");
    }
    result
}

/// ratatui's panic hook restores the screen but leaves mouse reporting on.
/// Installed before `ratatui::init`, which wraps it.
fn install_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
        let timeout = if app.game.animating() {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(200)
        };
        if !event::poll(timeout)? {
            continue;
        }
        // Coalesce bursts of mouse motion into one redraw. Stop after any other
        // event so clicks are always hit-tested against an up-to-date frame.
        loop {
            let ev = event::read()?;
            let motion = matches!(&ev, Event::Mouse(m) if m.kind == MouseEventKind::Moved);
            match ev {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                _ => {}
            }
            if !motion || !event::poll(Duration::ZERO)? {
                break;
            }
        }
    }
    Ok(())
}
