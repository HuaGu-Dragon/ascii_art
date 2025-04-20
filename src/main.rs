use std::{
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicPtr, mpsc::channel, Arc},
    thread,
};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen},
};
use image::GenericImageView;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

struct FrameData {
    width: u32,
    height: u32,
    //data: Vec<(u8, u8, u8, char)>,
    data: Vec<u8>,
}

struct DoubleBuffer {
    front: AtomicPtr<FrameData>,
    back: AtomicPtr<FrameData>,
    temp: AtomicPtr<FrameData>,
}

impl DoubleBuffer {
    fn new(width: u32, height: u32) -> Self {
        let front_box = Box::new(FrameData {
            width,
            height,
            data: Vec::with_capacity((width * height * 20) as usize),
        });
        let back_box = Box::new(FrameData {
            width,
            height,
            data: Vec::with_capacity((width * height * 20) as usize),
        });
        let temp_box = Box::new(FrameData {
            width,
            height,
            data: Vec::with_capacity((width * height * 20) as usize),
        });
        DoubleBuffer {
            front: AtomicPtr::new(Box::into_raw(front_box)),
            back: AtomicPtr::new(Box::into_raw(back_box)),
            temp: AtomicPtr::new(Box::into_raw(temp_box)),
        }
    }

    fn swap(&self) {
        let back_ptr = self.back.load(std::sync::atomic::Ordering::SeqCst);
        let front_ptr = self
            .front
            .swap(back_ptr, std::sync::atomic::Ordering::SeqCst);
        self.back
            .store(front_ptr, std::sync::atomic::Ordering::SeqCst);
    }

    fn front(&self) -> &FrameData {
        unsafe { &*self.front.load(std::sync::atomic::Ordering::SeqCst) }
    }
    fn temp_mut(&self) -> &mut FrameData {
        unsafe { &mut *self.temp.load(std::sync::atomic::Ordering::SeqCst) }
    }
    fn temp_to_back(&self) {
        let back_ptr = self.back.load(std::sync::atomic::Ordering::SeqCst);
        let temp_ptr = self
            .temp
            .swap(back_ptr, std::sync::atomic::Ordering::SeqCst);
        self.back
            .store(temp_ptr, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for DoubleBuffer {
    fn drop(&mut self) {
        let front_ptr = self.front.load(std::sync::atomic::Ordering::SeqCst);
        let back_ptr = self.back.load(std::sync::atomic::Ordering::SeqCst);
        let temp_ptr = self.temp.load(std::sync::atomic::Ordering::SeqCst);
        unsafe {
            drop(Box::from_raw(front_ptr));
            drop(Box::from_raw(back_ptr));
            drop(Box::from_raw(temp_ptr));
        }
    }
}

fn get_path() -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(4989);
    for i in 1..=4989 {
        let path = format!("target/images/{}.jpeg", i);
        paths.push(PathBuf::from(path));
    }
    paths
}

#[allow(dead_code)]
fn preload_images(paths: &[PathBuf]) -> Vec<image::DynamicImage> {
    let mut images = Vec::with_capacity(paths.len());
    for path in paths.iter() {
        let img = image::open(path).unwrap();
        images.push(img);
    }
    images
}

fn main() {
    let size = 5;
    let mut stdout = std::io::stdout();
    enable_raw_mode().unwrap();
    execute!(
        stdout,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )
    .unwrap();
    execute!(stdout, crossterm::cursor::Hide).unwrap();
    execute!(stdout, crossterm::cursor::MoveTo(0, 0)).unwrap();
    let paths = get_path();
    //let images = preload_images(&paths);
    let double_buffer = Arc::new(DoubleBuffer::new(1920 / size, 1080 / size));
    let (frame_ready_tx, frame_ready_rx) = channel();
    let (new_request_tx, new_request_rx) = channel();
    let db_cpu = Arc::clone(&double_buffer);
    let cpu_handle = thread::spawn(move || loop {
        for path in paths.iter() {
            //for img in images.iter() {
            let img = image::open(&path).unwrap();
            // new_request_rx.recv().unwrap();
            let back = db_cpu.temp_mut();
            {
                let cols = back.width;
                let rows = back.height;
                let chunk_rows = rows / 24;
                let blocks = (0..24)
                    .into_par_iter()
                    .map(|block_id| {
                        let start = block_id * chunk_rows;
                        let end = start + chunk_rows;
                        let mut buf = Vec::with_capacity(((end - start) * cols) as usize * 20);
                        for y in start..end {
                            for x in 0..cols {
                                let [r, g, b, _] = img.get_pixel(x, y).0;
                                write!(
                                    &mut buf,
                                    "\x1b[{};{}H\x1b[38;2;{};{};{}m{}",
                                    y,
                                    x * 2,
                                    r,
                                    g,
                                    b,
                                    "██"
                                )
                                .unwrap();
                            }
                        }
                        buf
                    })
                    .collect::<Vec<Vec<u8>>>();

                back.data.clear();
                for row in blocks {
                    back.data.extend(row);
                }
            }
            new_request_rx.recv().unwrap();
            db_cpu.temp_to_back();
            frame_ready_tx.send(()).unwrap();
        }
    });
    let db_render = Arc::clone(&double_buffer);
    let render_handle = thread::spawn(move || {
        new_request_tx.send(()).unwrap();
        // let mut write_buffer =
        //     Vec::with_capacity((db_render.front().width * db_render.front().height * 20) as usize);
        let mut stdout = std::io::stdout();
        let frame_time = std::time::Duration::from_millis(1000 / 16);
        let mut delay = std::time::Duration::ZERO;
        loop {
            let now = std::time::Instant::now();
            frame_ready_rx.recv().unwrap();
            {
                let front = db_render.front();
                stdout.write_all(&front.data).unwrap();
            }
            // Reset the cursor position
            execute!(stdout, crossterm::cursor::MoveTo(0, 0)).unwrap();
            let elapsed = now.elapsed();
            delay += elapsed;
            // 16 FPS
            if delay < frame_time {
                thread::sleep(frame_time - delay);
                delay = std::time::Duration::ZERO;
            } else {
                delay -= frame_time;
            }
            // Swap the buffers
            double_buffer.swap();
            // Notify the CPU thread that needs to process the new frame
            new_request_tx.send(()).unwrap();
        }
    });
    // Wait for the threads to finish
    cpu_handle.join().unwrap();
    render_handle.join().unwrap();

    disable_raw_mode().unwrap();
    execute!(stdout, LeaveAlternateScreen).unwrap();
    let (weight, height) = crossterm::terminal::size().unwrap();
    print!("{} {}", height, weight);
}
