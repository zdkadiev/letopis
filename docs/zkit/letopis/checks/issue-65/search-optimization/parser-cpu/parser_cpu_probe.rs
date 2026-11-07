use letopis::{domain::RepoPath, markdown};
use std::{env, fs, hint::black_box, time::Instant};

fn main() {
    let mut arguments = env::args().skip(1);
    let input = arguments.next().expect("input path");
    let iterations: usize = arguments.next().expect("iterations").parse().unwrap();
    let bytes = fs::read(input).unwrap();
    let path = RepoPath::parse("knowledge/part-0000.md").unwrap();
    let parsed = markdown::file_parse(&path, &bytes).unwrap().unwrap();
    let fingerprint = format!("{parsed:?}");
    fs::write(arguments.next().expect("fingerprint path"), fingerprint).unwrap();
    for _ in 0..10 {
        black_box(markdown::file_parse(black_box(&path), black_box(&bytes)).unwrap());
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(markdown::file_parse(black_box(&path), black_box(&bytes)).unwrap());
    }
    println!(
        "{{\"iterations\":{iterations},\"bytes\":{},\"elements\":{},\"elapsed_seconds\":{}}}",
        bytes.len(),
        parsed.elements.len(),
        start.elapsed().as_secs_f64()
    );
}
