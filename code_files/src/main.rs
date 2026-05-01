use raylib::prelude::*;
use rand::Rng;
use linux_embedded_hal::I2cdev;
use mpu6050::*;

// GAME SETTINGS 

const FPS: u32 = 60;
const POWERUP_DURATION: i32 = 360;


// MPU6050 SETTINGS


// MPU 1 -> Left Paddle -> Address 0x68
// MPU 2 -> Right Paddle -> Address 0x69

fn get_sensor_position(mpu: &mut Mpu6050<I2cdev>, screen_h: f32) -> f32 {
    match mpu.get_acc() {
        Ok(acc) => {
            // Using Y-axis tilt for paddle movement
            let tilt = acc.y;

            // Map tilt to screen position
            let mut pos = (screen_h / 2.0) + (tilt as f32 * 250.0);

            if pos < 50.0 {
                pos = 50.0;
            }

            if pos > screen_h - 150.0 {
                pos = screen_h - 150.0;
            }

            pos
        }
        Err(_) => screen_h / 2.0,
    }
}


// ENUMS & STRUCTS 

#[derive(Clone, Copy, PartialEq)]
enum PowerUpType {
    Bomb,
    Gem,
}

struct Paddle {
    rect: Rectangle,
    effect_timer: i32,
}

struct Ball {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    radius: f32,
    total_hits: i32,
    trail: Vec<Vector2>,
}

struct PowerUp {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    active: bool,
    kind: PowerUpType,
}


// LOGIC IMPLEMENTATIONS


impl Paddle {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            rect: Rectangle::new(x, y, w, h),
            effect_timer: 0,
        }
    }

    fn apply_powerup(&mut self, kind: PowerUpType, normal_h: f32) {
        self.effect_timer = POWERUP_DURATION;

        match kind {
            PowerUpType::Bomb => self.rect.height = normal_h * 0.63,
            PowerUpType::Gem => self.rect.height = normal_h * 1.54,
        }
    }

    fn update_effect(&mut self, normal_h: f32) {
        if self.effect_timer > 0 {
            self.effect_timer -= 1;

            if self.effect_timer == 0 {
                self.rect.height = normal_h;
            }
        }
    }
}

impl Ball {
    fn new(sw: f32, sh: f32) -> Self {
        let mut ball = Self {
            x: sw / 2.0,
            y: sh / 2.0,
            vx: sw * 0.005,
            vy: sw * 0.005,
            radius: sw * 0.0085,
            total_hits: 0,
            trail: vec![],
        };

        ball.reset(sw, sh);
        ball
    }

    fn reset(&mut self, sw: f32, sh: f32) {
        self.x = sw / 2.0;
        self.y = sh / 2.0;

        let mut rng = rand::thread_rng();
        let speed = sw * 0.005;

        self.vx = if rng.gen_bool(0.5) { speed } else { -speed };
        self.vy = if rng.gen_bool(0.5) { speed } else { -speed };

        self.trail.clear();
        self.total_hits = 0;
    }
}

// COLLISION


fn check_col(cx: f32, cy: f32, r: f32, rect: Rectangle) -> bool {
    let tx = cx.clamp(rect.x, rect.x + rect.width);
    let ty = cy.clamp(rect.y, rect.y + rect.height);

    ((cx - tx).powi(2) + (cy - ty).powi(2)) <= r.powi(2)
}

// GAME LOOP 

fn game_loop(
    rl: &mut RaylibHandle,
    thread: &RaylibThread,
    mut mpu1: Mpu6050<I2cdev>,
    mut mpu2: Mpu6050<I2cdev>,
) {
    let sw = rl.get_screen_width() as f32;
    let sh = rl.get_screen_height() as f32;

    let top_b = sh * 0.025;
    let bot_b = sh * 0.95;
    let p_w = sw * 0.01;
    let p_h_norm = sh * 0.137;

    let mut left = Paddle::new(
        sw * 0.035,
        sh / 2.0 - p_h_norm / 2.0,
        p_w,
        p_h_norm,
    );

    let mut right = Paddle::new(
        sw * 0.95,
        sh / 2.0 - p_h_norm / 2.0,
        p_w,
        p_h_norm,
    );

    let mut ball = Ball::new(sw, sh);

    let mut l_score = 0;
    let mut r_score = 0;

    loop {
        if rl.window_should_close() {
            break;
        }

        
        // MPU6050 CONTROL HERE

        left.rect.y = get_sensor_position(&mut mpu1, sh);
        right.rect.y = get_sensor_position(&mut mpu2, sh);

        // Keep inside bounds
        left.rect.y = left.rect.y.clamp(top_b, bot_b - left.rect.height);
        right.rect.y = right.rect.y.clamp(top_b, bot_b - right.rect.height);

        
        // BALL PHYSICS
    

        ball.trail.push(Vector2::new(ball.x, ball.y));
        if ball.trail.len() > 8 {
            ball.trail.remove(0);
        }

        ball.x += ball.vx;
        ball.y += ball.vy;

        if ball.y - ball.radius <= top_b || ball.y + ball.radius >= bot_b {
            ball.vy *= -1.0;
        }

        if check_col(ball.x, ball.y, ball.radius, left.rect) && ball.vx < 0.0 {
            ball.vx *= -1.1;
            l_score += 1;
        }

        if check_col(ball.x, ball.y, ball.radius, right.rect) && ball.vx > 0.0 {
            ball.vx *= -1.1;
            r_score += 1;
        }

        if ball.x <= 0.0 || ball.x >= sw {
            ball.reset(sw, sh);
        }

        
        // DRAWING
        

        let mut d = rl.begin_drawing(thread);
        d.clear_background(Color::BLACK);

        d.draw_line(0, top_b as i32, sw as i32, top_b as i32, Color::WHITE);
        d.draw_line(0, bot_b as i32, sw as i32, bot_b as i32, Color::WHITE);

        for y in (20..(sh as i32 - 20)).step_by(45) {
            d.draw_rectangle((sw / 2.0) as i32, y, 8, 28, Color::WHITE);
        }

        d.draw_text(
            &l_score.to_string(),
            (sw * 0.39) as i32,
            (sh * 0.075) as i32,
            (sh * 0.068) as i32,
            Color::WHITE,
        );

        d.draw_text(
            &r_score.to_string(),
            (sw * 0.6) as i32,
            (sh * 0.075) as i32,
            (sh * 0.068) as i32,
            Color::WHITE,
        );

        d.draw_rectangle_rec(left.rect, Color::WHITE);
        d.draw_rectangle_rec(right.rect, Color::WHITE);

        for p in &ball.trail {
            d.draw_circle_v(*p, ball.radius - 4.0, Color::GRAY);
        }

        d.draw_circle(
            ball.x as i32,
            ball.y as i32,
            ball.radius,
            Color::WHITE,
        );
    }
}

// MAIN

fn main() {
    
    // MPU6050 INIT
    

    let dev1 = I2cdev::new("/dev/i2c-1").unwrap();
    let dev2 = I2cdev::new("/dev/i2c-1").unwrap();

    let mut mpu1 = Mpu6050::new(dev1);
    let mut mpu2 = Mpu6050::new(dev2);

    // Sensor 1 -> default 0x68
    mpu1.init().unwrap();

    // Sensor 2 -> address 0x69
    mpu2.set_slave_address(0x69);
    mpu2.init().unwrap();

    println!("MPU6050 sensors initialized successfully!");


    // RAYLIB INIT
    

    let (mut rl, thread) = raylib::init()
        .size(0, 0)
        .fullscreen()
        .title("PONG + MPU6050")
        .build();

    rl.set_target_fps(FPS);

    game_loop(&mut rl, &thread, mpu1, mpu2);
}
