pub(super) fn transform(real: &mut [f32], imaginary: &mut [f32], size: usize) {
    assert!(
        size.is_power_of_two(),
        "a Fourier transform runs on a power of two"
    );
    assert_eq!(
        real.len(),
        size * size,
        "a Fourier transform covers a square image"
    );
    assert_eq!(
        imaginary.len(),
        size * size,
        "a Fourier transform covers a square image"
    );
    let table = twiddles(size);
    for row in 0..size {
        let offset = row * size;
        line(
            &mut real[offset..offset + size],
            &mut imaginary[offset..offset + size],
            &table,
        );
    }
    let mut held_real = vec![0.0f32; size];
    let mut held_imaginary = vec![0.0f32; size];
    for column in 0..size {
        for (row, (sample_real, sample_imaginary)) in held_real
            .iter_mut()
            .zip(held_imaginary.iter_mut())
            .enumerate()
        {
            let index = column + row * size;
            *sample_real = real[index];
            *sample_imaginary = imaginary[index];
        }
        line(&mut held_real, &mut held_imaginary, &table);
        for (row, (sample_real, sample_imaginary)) in
            held_real.iter().zip(held_imaginary.iter()).enumerate()
        {
            let index = column + row * size;
            real[index] = *sample_real;
            imaginary[index] = *sample_imaginary;
        }
    }
}

fn line(real: &mut [f32], imaginary: &mut [f32], table: &[(f32, f32)]) {
    let size = real.len();
    let mut reversed = 0usize;
    for index in 1..size {
        let mut bit = size >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            real.swap(index, reversed);
            imaginary.swap(index, reversed);
        }
    }
    let mut length = 2usize;
    while length <= size {
        let step = size / length;
        let half = length / 2;
        let mut start = 0usize;
        while start < size {
            for offset in 0..half {
                let (cosine, sine) = table[offset * step];
                let lower = start + offset;
                let upper = lower + half;
                let source_real = real[upper];
                let source_imaginary = imaginary[upper];
                let turned_real = source_real * cosine - source_imaginary * sine;
                let turned_imaginary = source_real * sine + source_imaginary * cosine;
                real[upper] = real[lower] - turned_real;
                imaginary[upper] = imaginary[lower] - turned_imaginary;
                real[lower] += turned_real;
                imaginary[lower] += turned_imaginary;
            }
            start += length;
        }
        length <<= 1;
    }
}

fn twiddles(size: usize) -> Vec<(f32, f32)> {
    (0..size / 2)
        .map(|index| {
            let angle = -2.0 * std::f64::consts::PI * index as f64 / size as f64;
            (angle.cos() as f32, angle.sin() as f32)
        })
        .collect()
}
