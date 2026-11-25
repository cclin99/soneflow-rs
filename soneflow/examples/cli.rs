use std::env;
use std::error::Error;
use std::path::PathBuf;

use hound::{SampleFormat, WavReader};
use soneflow::{loudness_zwst, FieldType};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let (path, calib) = parse_args(&args)?;
    let (signal, fs) = read_wav_mono(path, calib)?;
    let result = loudness_zwst(&signal, fs as f64, FieldType::Free)?;
    println!("N = {:.6} sone", result.n);
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(PathBuf, f64), Box<dyn Error>> {
    if args.is_empty() {
        return Err("usage: cli <wav-path> [--calib <float>]".into());
    }

    let path = PathBuf::from(&args[0]);
    let mut calib = 1.0;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--calib" => {
                let value = args.get(i + 1).ok_or("--calib requires a value")?;
                calib = value.parse::<f64>()?;
                if !calib.is_finite() {
                    return Err("--calib must be finite".into());
                }
                i += 2;
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok((path, calib))
}

fn read_wav_mono(path: PathBuf, calib: f64) -> Result<(Vec<f64>, u32), Box<dyn Error>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 {
        return Err("WAV file has zero channels".into());
    }

    let samples = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map(f64::from))
            .collect::<Result<Vec<_>, _>>()?,
        SampleFormat::Int => {
            let bits = u32::from(spec.bits_per_sample);
            if bits == 0 || bits > 32 {
                return Err(format!("unsupported integer WAV bit depth: {bits}").into());
            }
            let scale = (1_u64 << (bits - 1)) as f64;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| f64::from(v) / scale))
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    let mono = if channels == 1 {
        samples.into_iter().map(|s| s * calib).collect()
    } else {
        samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f64>() * calib / channels as f64)
            .collect()
    };

    Ok((mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use std::path::PathBuf;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn err(list: &[&str]) -> String {
        parse_args(&args(list)).unwrap_err().to_string()
    }

    #[test]
    fn path_alone_uses_unit_calibration() {
        let (path, calib) = parse_args(&args(&["a.wav"])).unwrap();
        assert_eq!(path, PathBuf::from("a.wav"));
        assert_eq!(calib, 1.0);
    }

    #[test]
    fn calib_value_is_parsed_and_last_one_wins() {
        let (_, calib) = parse_args(&args(&["a.wav", "--calib", "0.5"])).unwrap();
        assert_eq!(calib, 0.5);
        let (_, calib) = parse_args(&args(&["a.wav", "--calib", "2", "--calib", "-3e-1"])).unwrap();
        assert_eq!(calib, -0.3);
    }

    #[test]
    fn missing_path_prints_usage() {
        assert!(err(&[]).starts_with("usage: cli <wav-path>"));
    }

    #[test]
    fn calib_without_value_is_rejected() {
        assert_eq!(err(&["a.wav", "--calib"]), "--calib requires a value");
    }

    #[test]
    fn calib_must_parse_and_be_finite() {
        assert!(parse_args(&args(&["a.wav", "--calib", "x"])).is_err());
        for bad in ["inf", "-inf", "NaN"] {
            assert_eq!(err(&["a.wav", "--calib", bad]), "--calib must be finite");
        }
    }

    #[test]
    fn unknown_argument_is_rejected() {
        assert_eq!(err(&["a.wav", "--gain", "2"]), "unknown argument: --gain");
    }
}
