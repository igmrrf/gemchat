use std::sync::{Arc, Mutex};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem, Child};
use tui_term::vt100::Parser;
use std::io::{Read, Write};

impl Drop for EmbeddedTerminal {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

pub struct EmbeddedTerminal {
    pub writer: Mutex<Box<dyn Write + Send>>,
    pub parser: Arc<Mutex<Parser>>,
    pub child: Box<dyn Child + Send>,
}

impl EmbeddedTerminal {
    pub fn new(command: &str, cwd: &std::path::Path, rows: u16, cols: u16) -> color_eyre::Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }).map_err(|e| color_eyre::eyre::eyre!("{:?}", e))?;

        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg(command);
        cmd.cwd(cwd);
        
        let child = pair.slave.spawn_command(cmd).map_err(|e| color_eyre::eyre::eyre!("{:?}", e))?;
        
        let parser = Arc::new(Mutex::new(Parser::new(rows, cols, 0)));
        
        let master = pair.master;
        let writer = master.take_writer().map_err(|e| color_eyre::eyre::eyre!("{:?}", e))?;
        let mut reader = master.try_clone_reader().map_err(|e| color_eyre::eyre::eyre!("{:?}", e))?;
        let parser_clone = Arc::clone(&parser);
        
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                let mut p = parser_clone.lock().unwrap();
                p.process(&buf[..n]);
            }
        });

        Ok(Self {
            writer: Mutex::new(writer),
            parser,
            child,
        })
    }

    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()?;
        Ok(())
    }

    pub fn screen_text(&self) -> String {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        let (rows, cols) = screen.size();
        let mut text = String::new();
        for r in 0..rows {
            for c in 0..cols {
                if let Some(cell) = screen.cell(r, c) {
                    text.push_str(cell.contents());
                }
            }
            text.push('\n');
        }
        text
    }
}
