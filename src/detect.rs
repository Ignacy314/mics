use std::{fs::File, io::BufReader, path::Path};

use convolutions_rs::convolutions::ConvolutionLayer;
use ndarray::{Array, Array3, Array4};
use smartcore::{
    ensemble::random_forest_classifier::RandomForestClassifier, linalg::basic::matrix::DenseMatrix,
};
use spectrum_analyzer::{samples_fft_to_spectrum, windows::hann_window};

pub fn process_samples(samples: &[i32]) -> (Vec<f32>, Vec<f32>) {
    let samples = samples.iter().map(|s| *s as f32).collect::<Vec<_>>();
    let hann_window = hann_window(&samples);

    let spectrum = samples_fft_to_spectrum(
        &hann_window,
        48000,
        spectrum_analyzer::FrequencyLimit::Range(5.0, 4000.0),
        // spectrum_analyzer::FrequencyLimit::All,
        None,
    )
    .unwrap();

    // let frequencies: Vec<f32> = (5..=4000).map(|s| s as f32).collect();

    let (freqs, values): (Vec<_>, Vec<_>) = spectrum.data().iter().copied().unzip();
    let freqs: Vec<f32> = freqs.into_iter().map(|f| f.val()).collect();

    let values: Vec<f32> = values.iter().map(|s| s.val().abs()).collect();
    let input = Array::from_shape_vec((1, 1, values.len()), values.clone()).unwrap();
    let kernel: Array4<f32> = Array::from_shape_vec((1, 1, 1, 21), vec![1.0 / 21.0; 21]).unwrap();
    let conv_layer = ConvolutionLayer::new(kernel, None, 1, convolutions_rs::Padding::Same);
    let output_layer: Array3<f32> = conv_layer.convolve(&input);
    let output_layer = output_layer.into_raw_vec();

    let mut fft_diff = values
        .iter()
        .zip(output_layer.iter())
        .map(|(v, a)| v - a)
        .collect::<Vec<f32>>();
    let min_diff = *fft_diff.iter().min_by(|a, b| a.total_cmp(b)).unwrap();
    let max_diff = *fft_diff.iter().max_by(|a, b| a.total_cmp(b)).unwrap();

    if max_diff > min_diff {
        fft_diff
            .iter_mut()
            .for_each(|s| *s = 2.0 * (*s - min_diff) / (max_diff - min_diff) - 1.0);
    } else {
        fft_diff = vec![0.0; fft_diff.len()];
    }

    // let interp_fft_diff = interp_slice(&freqs, &fft_diff, &frequencies, &InterpMode::default());
    // assert_eq!(interp_fft_diff.len(), 3996);

    (freqs, fft_diff)
}

pub fn load_model<P: AsRef<Path>>(
    model_path: P,
) -> RandomForestClassifier<f32, i32, DenseMatrix<f32>, Vec<i32>> {
    bincode::deserialize_from(BufReader::new(File::open(model_path).unwrap())).unwrap()
}
