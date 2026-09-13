// The same work as `dispatch.skuld`, in Rust. See `README.md`.

trait Shape {
    fn area(&self) -> i64;
}

struct Square {
    side: i64,
}
struct Rectangle {
    width: i64,
    height: i64,
}
struct Triangle {
    base: i64,
    height: i64,
}

impl Shape for Square {
    fn area(&self) -> i64 {
        self.side * self.side
    }
}
impl Shape for Rectangle {
    fn area(&self) -> i64 {
        self.width * self.height
    }
}
impl Shape for Triangle {
    fn area(&self) -> i64 {
        self.base * self.height / 2
    }
}

fn main() {
    let count: i64 = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let mut shapes: Vec<Box<dyn Shape>> = Vec::new();
    for index in 0..count {
        match index % 3 {
            0 => shapes.push(Box::new(Square { side: index % 100 })),
            1 => shapes.push(Box::new(Rectangle {
                width: index % 50,
                height: index % 20,
            })),
            _ => shapes.push(Box::new(Triangle {
                base: index % 40,
                height: index % 30,
            })),
        }
    }
    let mut total: i64 = 0;
    for shape in &shapes {
        total = (total + shape.area()) % 1000000007;
    }
    println!("{total}");
}
