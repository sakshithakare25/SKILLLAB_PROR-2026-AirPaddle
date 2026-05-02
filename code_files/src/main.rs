#![allow(dead_code)]

use raylib::prelude::*;
use rand::Rng;
use rppal::i2c::I2c;
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ============================================================
// SETTINGS
// ============================================================
const FPS: u32          = 60;
const POWERUP_DURATION: i32 = 360;
const POWERUP_INTERVAL: i32 = 300;

// ============================================================
// MPU6050
// ============================================================
const MPU1_ADDR: u16   = 0x68;
const MPU2_ADDR: u16   = 0x69;
const PWR_MGMT_1: u8   = 0x6B;
const ACCEL_XOUT_H: u8 = 0x3B;
const ACC_SCALE: f32   = 16384.0;
const SMOOTH: f32      = 0.65;

fn init_mpu(i2c: &mut I2c, addr: u16) {
    i2c.set_slave_address(addr).expect("set_slave_address failed");
    i2c.smbus_write_byte(PWR_MGMT_1, 0).expect("MPU wake failed");
}

fn read_roll_raw(i2c: &mut I2c, addr: u16) -> Result<f32, rppal::i2c::Error> {
    i2c.set_slave_address(addr)?;
    let mut buf = [0u8; 6];
    i2c.write_read(&[ACCEL_XOUT_H], &mut buf)?;
    let ay = i16::from_be_bytes([buf[2], buf[3]]) as f32 / ACC_SCALE;
    let az = i16::from_be_bytes([buf[4], buf[5]]) as f32 / ACC_SCALE;
    Ok(ay.atan2(az))
}

fn roll_to_y(roll: f32, sh: f32, paddle_h: f32, top_b: f32, bot_b: f32) -> f32 {
    let t      = (roll / (PI / 2.0)).clamp(-1.0, 1.0);
    let center = (top_b + bot_b) / 2.0 - paddle_h / 2.0;
    let travel = (bot_b - top_b - paddle_h) * 0.48;
    (center + t * travel).clamp(top_b, bot_b - paddle_h)
}

// ============================================================
// SENSOR STATE  (shared between sensor thread & game thread)
// ============================================================
#[derive(Clone, Copy)]
struct SensorState {
    roll1: f32,
    roll2: f32,
}

/// Spawns a background thread that reads both MPUs at ~100 Hz
/// and updates the shared state. The game loop just reads the
/// latest value without ever blocking on I2C.
fn start_sensor_thread(state: Arc<Mutex<SensorState>>) {
    thread::spawn(move || {
        // Open a dedicated I2C handle for the sensor thread
        let mut i2c = I2c::new().expect("sensor thread: failed to open I2C");
        init_mpu(&mut i2c, MPU1_ADDR);
        init_mpu(&mut i2c, MPU2_ADDR);

        let mut roll1_s = 0.0f32;
        let mut roll2_s = 0.0f32;

        loop {
            if let Ok(r) = read_roll_raw(&mut i2c, MPU1_ADDR) {
                roll1_s = SMOOTH * roll1_s + (1.0 - SMOOTH) * r;
            }
            if let Ok(r) = read_roll_raw(&mut i2c, MPU2_ADDR) {
                roll2_s = SMOOTH * roll2_s + (1.0 - SMOOTH) * r;
            }

            if let Ok(mut s) = state.lock() {
                s.roll1 = roll1_s;
                s.roll2 = roll2_s;
            }

            // 100 Hz sensor loop — fast enough, won't fight the game loop
            thread::sleep(Duration::from_millis(10));
        }
    });
}

// ============================================================
// STRUCTS
// ============================================================
#[derive(Clone, Copy, PartialEq)]
enum PowerUpType { Bomb, Gem }

struct Paddle {
    rect: Rectangle,
    effect_timer: i32,
}

struct Ball {
    x: f32, y: f32,
    vx: f32, vy: f32,
    radius: f32,
    total_hits: i32,
    trail: Vec<Vector2>,
}

struct PowerUp {
    x: f32, y: f32,
    vx: f32, vy: f32,
    active: bool,
    kind: PowerUpType,
}

// ============================================================
// IMPLEMENTATIONS
// ============================================================
impl Paddle {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { rect: Rectangle::new(x, y, w, h), effect_timer: 0 }
    }
    fn move_up(&mut self, top: f32, speed: f32) {
        if self.rect.y > top { self.rect.y -= speed; }
    }
    fn move_down(&mut self, bottom: f32, speed: f32) {
        if self.rect.y + self.rect.height < bottom { self.rect.y += speed; }
    }
    fn apply_powerup(&mut self, kind: PowerUpType, normal_h: f32) {
        self.effect_timer = POWERUP_DURATION;
        self.rect.height = match kind {
            PowerUpType::Bomb => normal_h * 0.63,
            PowerUpType::Gem  => normal_h * 1.54,
        };
    }
    fn update_effect(&mut self, normal_h: f32) {
        if self.effect_timer > 0 {
            self.effect_timer -= 1;
            if self.effect_timer == 0 { self.rect.height = normal_h; }
        }
    }
}

impl Ball {
    fn new(sw: f32, sh: f32) -> Self {
        let mut b = Self {
            x: sw/2.0, y: sh/2.0, vx: 0.0, vy: 0.0,
            radius: sw * 0.0085, total_hits: 0, trail: vec![],
        };
        b.reset(sw, sh);
        b
    }
    fn reset(&mut self, sw: f32, sh: f32) {
        self.x = sw/2.0; self.y = sh/2.0;
        let mut rng = rand::thread_rng();
        let spd = sw * 0.005;
        self.vx = if rng.gen_bool(0.5) { spd } else { -spd };
        self.vy = if rng.gen_bool(0.5) { spd } else { -spd };
        self.trail.clear();
        self.total_hits = 0;
    }
}

fn check_col(cx: f32, cy: f32, r: f32, rect: Rectangle) -> bool {
    let tx = cx.clamp(rect.x, rect.x + rect.width);
    let ty = cy.clamp(rect.y, rect.y + rect.height);
    (cx - tx).powi(2) + (cy - ty).powi(2) <= r.powi(2)
}

// ============================================================
// HOME SCREEN
// ============================================================
fn home_screen(rl: &mut RaylibHandle, thread: &RaylibThread) -> bool {
    loop {
        let sw = rl.get_screen_width()  as f32;
        let sh = rl.get_screen_height() as f32;
        if rl.window_should_close() { std::process::exit(0); }
        if rl.is_key_pressed(KeyboardKey::KEY_ONE) { return true;  }
        if rl.is_key_pressed(KeyboardKey::KEY_TWO) { return false; }

        let mut d = rl.begin_drawing(thread);
        d.clear_background(Color::BLACK);
        for y in (20..(sh as i32 - 20)).step_by(45) {
            d.draw_rectangle((sw/2.0) as i32, y, 8, 28, Color::WHITE);
        }
        d.draw_text("PONG",    (sw*0.357) as i32, (sh*0.11) as i32, (sh*0.11) as i32, Color::WHITE);
        d.draw_text("CLASSIC", (sw*0.300) as i32, (sh*0.26) as i32, (sh*0.11) as i32, Color::WHITE);

        d.draw_text("1 PLAYER",    (sw*0.10) as i32, (sh*0.53) as i32, (sh*0.068) as i32, Color::WHITE);
        d.draw_text("Sensor + AI", (sw*0.11) as i32, (sh*0.63) as i32, (sh*0.037) as i32, Color::GRAY);
        d.draw_text("Press  1",    (sw*0.17) as i32, (sh*0.70) as i32, (sh*0.043) as i32, Color::GRAY);

        d.draw_text("2 PLAYERS",    (sw*0.63) as i32, (sh*0.53) as i32, (sh*0.068) as i32, Color::WHITE);
        d.draw_text("Both Sensors", (sw*0.64) as i32, (sh*0.63) as i32, (sh*0.037) as i32, Color::GRAY);
        d.draw_text("Press  2",     (sw*0.73) as i32, (sh*0.70) as i32, (sh*0.043) as i32, Color::GRAY);

        d.draw_text("Tilt sensor to move paddle", (sw*0.27) as i32, (sh*0.84) as i32, (sh*0.037) as i32, Color::GRAY);
        d.draw_text("ESC to Quit",                (sw*0.38) as i32, (sh*0.91) as i32, (sh*0.037) as i32, Color::GRAY);
    }
}

// ============================================================
// GAME OVER SCREEN
// ============================================================
fn game_over_screen(
    rl: &mut RaylibHandle, thread: &RaylibThread,
    winner: &str, l_score: i32, r_score: i32,
) -> i32 {
    loop {
        let sw = rl.get_screen_width()  as f32;
        let sh = rl.get_screen_height() as f32;
        if rl.window_should_close() { std::process::exit(0); }
        if rl.is_key_pressed(KeyboardKey::KEY_R) { return 1; }
        if rl.is_key_pressed(KeyboardKey::KEY_H) { return 2; }

        let mut d = rl.begin_drawing(thread);
        d.clear_background(Color::BLACK);
        d.draw_text(&format!("{} WINS!", winner),
            (sw*0.21) as i32, (sh*0.16) as i32, (sh*0.087) as i32, Color::WHITE);
        d.draw_text(&format!("FINAL SCORE : {} - {}", l_score, r_score),
            (sw*0.25) as i32, (sh*0.43) as i32, (sh*0.052) as i32, Color::WHITE);
        d.draw_text("Press R to Restart", (sw*0.30) as i32, (sh*0.58) as i32, (sh*0.047) as i32, Color::WHITE);
        d.draw_text("Press H for Home",   (sw*0.26) as i32, (sh*0.67) as i32, (sh*0.047) as i32, Color::WHITE);
    }
}

// ============================================================
// GAME LOOP
// ============================================================
fn game_loop(
    rl: &mut RaylibHandle,
    thread: &RaylibThread,
    sensors: Arc<Mutex<SensorState>>,
    single_player: bool,
) {
    let sw = rl.get_screen_width()  as f32;
    let sh = rl.get_screen_height() as f32;

    let top_b    = sh * 0.025;
    let bot_b    = sh * 0.95;
    let p_w      = sw * 0.012;
    let p_h_norm = sh * 0.137;
    let p_speed  = sh * 0.012;

    let mut left  = Paddle::new(sw*0.035, sh/2.0 - p_h_norm/2.0, p_w, p_h_norm);
    let mut right = Paddle::new(sw*0.950, sh/2.0 - p_h_norm/2.0, p_w, p_h_norm);
    let mut ball  = Ball::new(sw, sh);
    let mut powerup = PowerUp { x:0.0, y:0.0, vx:0.0, vy:0.0, active:false, kind:PowerUpType::Gem };

    let mut l_score = 0i32;
    let mut r_score = 0i32;
    let mut frames  = 0i32;
    let max_spd     = sw * 0.018;

    loop {
        if rl.window_should_close() { break; }
        frames += 1;

        // ── Read latest sensor values (never blocks — just reads shared state) ──
        let (roll1, roll2) = {
            let s = sensors.lock().unwrap();
            (s.roll1, s.roll2)
        };

        // ── Paddle positions ──────────────────────────────────────────────────
        left.rect.y = roll_to_y(roll1, sh, left.rect.height, top_b, bot_b);

        if single_player {
            let center = right.rect.y + right.rect.height / 2.0;
            if      center < ball.y - 4.0 { right.move_down(bot_b, p_speed); }
            else if center > ball.y + 4.0 { right.move_up(top_b,  p_speed); }
        } else {
            right.rect.y = roll_to_y(roll2, sh, right.rect.height, top_b, bot_b);
        }

        left.update_effect(p_h_norm);
        right.update_effect(p_h_norm);

        // ── Power-up spawn ────────────────────────────────────────────────────
        if frames % POWERUP_INTERVAL == 0 && !powerup.active {
            let mut rng = rand::thread_rng();
            powerup.active = true;
            powerup.x    = sw / 2.0;
            powerup.y    = rng.gen_range((top_b + 80.0)..(bot_b - 80.0));
            powerup.vx   = if rng.gen_bool(0.5) { sw*0.004 } else { -sw*0.004 };
            powerup.vy   = rng.gen_range(-(sh*0.003)..(sh*0.003));
            powerup.kind = if rng.gen_bool(0.5) { PowerUpType::Bomb } else { PowerUpType::Gem };
        }

        // ── Ball physics ──────────────────────────────────────────────────────
        ball.trail.push(Vector2::new(ball.x, ball.y));
        if ball.trail.len() > 6 { ball.trail.remove(0); }
        ball.x += ball.vx;
        ball.y += ball.vy;

        if ball.y - ball.radius <= top_b || ball.y + ball.radius >= bot_b { ball.vy *= -1.0; }

        if check_col(ball.x, ball.y, ball.radius, left.rect) && ball.vx < 0.0 {
            ball.vx = (ball.vx * -1.1).clamp(-max_spd, max_spd);
            l_score += 1; ball.total_hits += 1;
        }
        if check_col(ball.x, ball.y, ball.radius, right.rect) && ball.vx > 0.0 {
            ball.vx = (ball.vx * -1.1).clamp(-max_spd, max_spd);
            r_score += 1; ball.total_hits += 1;
        }
        ball.vy = ball.vy.clamp(-max_spd, max_spd);

        // ── Power-up logic ────────────────────────────────────────────────────
        if powerup.active {
            powerup.x += powerup.vx;
            powerup.y += powerup.vy;
            if powerup.y <= top_b || powerup.y >= bot_b { powerup.vy *= -1.0; }
            if check_col(powerup.x, powerup.y, 15.0, left.rect)  { left.apply_powerup(powerup.kind,  p_h_norm); powerup.active = false; }
            if check_col(powerup.x, powerup.y, 15.0, right.rect) { right.apply_powerup(powerup.kind, p_h_norm); powerup.active = false; }
            if powerup.x < 0.0 || powerup.x > sw { powerup.active = false; }
        }

        // ── Win check ─────────────────────────────────────────────────────────
        if ball.x <= 0.0 {
            let win = if single_player { "COMPUTER" } else { "RIGHT PLAYER" };
            match game_over_screen(rl, thread, win, l_score, r_score) {
                1 => { game_loop(rl, thread, sensors, single_player); return; }
                _ => return,
            }
        }
        if ball.x >= sw {
            match game_over_screen(rl, thread, "LEFT PLAYER", l_score, r_score) {
                1 => { game_loop(rl, thread, sensors, single_player); return; }
                _ => return,
            }
        }

        // ── Draw ──────────────────────────────────────────────────────────────
        let mut d = rl.begin_drawing(thread);
        d.clear_background(Color::BLACK);

        d.draw_line(0, top_b as i32, sw as i32, top_b as i32, Color::WHITE);
        d.draw_line(0, bot_b as i32, sw as i32, bot_b as i32, Color::WHITE);
        for y in (20..(sh as i32 - 20)).step_by(45) {
            d.draw_rectangle((sw/2.0) as i32, y, 8, 28, Color::WHITE);
        }

        d.draw_text(&l_score.to_string(), (sw*0.39) as i32, (sh*0.075) as i32, (sh*0.068) as i32, Color::WHITE);
        d.draw_text(&r_score.to_string(), (sw*0.60) as i32, (sh*0.075) as i32, (sh*0.068) as i32, Color::WHITE);

        let lc = if left.effect_timer  > 0 { Color::YELLOW } else { Color::WHITE };
        let rc = if right.effect_timer > 0 { Color::YELLOW } else { Color::WHITE };
        d.draw_rectangle_rec(left.rect,  lc);
        d.draw_rectangle_rec(right.rect, rc);

        let tlen = ball.trail.len();
        for (i, p) in ball.trail.iter().enumerate() {
            let a = ((i + 1) as f32 / tlen as f32 * 160.0) as u8;
            d.draw_circle_v(*p, ball.radius - 3.0, Color { r: 150, g: 150, b: 150, a });
        }
        d.draw_circle(ball.x as i32, ball.y as i32, ball.radius, Color::WHITE);

        if powerup.active {
            match powerup.kind {
                PowerUpType::Bomb => {
                    d.draw_circle(powerup.x as i32, powerup.y as i32, 16.0, Color::RED);
                    d.draw_text("!", (powerup.x - 5.0) as i32, (powerup.y - 12.0) as i32, 24, Color::WHITE);
                }
                PowerUpType::Gem => {
                    d.draw_poly(Vector2::new(powerup.x, powerup.y), 4, 18.0, 45.0, Color::GOLD);
                    for angle in [0.0f32, 90.0, 180.0, 270.0] {
                        let rad = angle.to_radians();
                        d.draw_circle(
                            (powerup.x + rad.cos() * 22.0) as i32,
                            (powerup.y + rad.sin() * 22.0) as i32,
                            3.0, Color::YELLOW,
                        );
                    }
                }
            }
            let (label, col) = match powerup.kind {
                PowerUpType::Gem  => ("DIAMOND - Catch to GROW paddle",  Color::GOLD),
                PowerUpType::Bomb => ("BOMB    - Catch to SHRINK paddle", Color::RED),
            };
            d.draw_text(label, (sw*0.28) as i32, (sh*0.965) as i32, (sh*0.030) as i32, col);
        }
    }
}

// ============================================================
// MAIN
// ============================================================
fn main() {
    // Shared sensor state — initialised to zero (flat sensor)
    let sensor_state = Arc::new(Mutex::new(SensorState { roll1: 0.0, roll2: 0.0 }));

    // Sensor thread reads I2C at 100 Hz in background
    start_sensor_thread(Arc::clone(&sensor_state));
    println!("Sensor thread started (MPU1=0x{:X}, MPU2=0x{:X})", MPU1_ADDR, MPU2_ADDR);

    // Give sensors a moment to settle
    thread::sleep(Duration::from_millis(200));

    let (mut rl, thread) = raylib::init()
        .size(0, 0)
        .fullscreen()
        .title("PONG CLASSIC")
        .build();

    rl.set_target_fps(FPS);

    loop {
        let single_player = home_screen(&mut rl, &thread);
        game_loop(&mut rl, &thread, Arc::clone(&sensor_state), single_player);
    }
}
