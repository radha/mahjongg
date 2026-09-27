mod app;
mod game;
mod map;
mod rng;
mod storage;
mod theme;
mod ui;

use std::io::stdout;
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;

use app::App;

fn main() -> std::io::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("mahjongg {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut app = App::new();
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;

    let result = run(&mut terminal, &mut app);

    app.shutdown();
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
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
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                _ => {}
            }
        }
    }
    Ok(())
}
