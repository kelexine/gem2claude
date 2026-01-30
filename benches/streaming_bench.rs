use criterion::{black_box, criterion_group, criterion_main, Criterion};
use gem2claude::translation::stream_processors::{process_text_segment, BlockType};

fn bench_process_text_segment(c: &mut Criterion) {
    let text =
        "Here is some text with a <think>thinking block</think> inside it. And some more text.";

    c.bench_function("process_text_segment_mixed", |b| {
        b.iter(|| {
            let mut in_thinking = false;
            let mut buffer = String::new();
            let _ = process_text_segment(black_box(text), &mut in_thinking, &mut buffer);
        })
    });
}

fn bench_process_text_segment_split(c: &mut Criterion) {
    let chunk1 = "Here is a start <thi";
    let chunk2 = "nk> and the rest.";

    c.bench_function("process_text_segment_split_tag", |b| {
        b.iter(|| {
            let mut in_thinking = false;
            let mut buffer = String::new();

            // Simulate processing split chunks (stateful)
            let _ = process_text_segment(black_box(chunk1), &mut in_thinking, &mut buffer);
            let _ = process_text_segment(black_box(chunk2), &mut in_thinking, &mut buffer);
        })
    });
}

criterion_group!(
    benches,
    bench_process_text_segment,
    bench_process_text_segment_split
);
criterion_main!(benches);
