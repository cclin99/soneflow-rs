#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "unsafe blocks from 0.1.0 without SAFETY comments; remove once documented"
)]
use super::levels::levels_with_backend;
use super::nl_unit::{CtxNlHooks, NoNlHooks};
use super::nonlinear::{nl_band, nonlinear_with_backend};
use super::shared::StageCtx;
use super::temporal::{temporal_with, CtxTwHooks, NoTwHooks};
use super::*;
use crate::error::{NumericFaultStage as Stage, SoneflowError};
use crate::zwtv::nonlinear_decay::{nl_coeffs, NlInit};
use crate::zwtv::par::ParMode;

/// Caller MXCSR qualified, core not validated (the per-frame checks run).
const UNVALIDATED: StageCtx = StageCtx {
    qualified: true,
    validated: false,
    avx2: false,
};
fn bits(x: &[f64]) -> Vec<u64> {
    x.iter().map(|v| v.to_bits()).collect()
}

/// The product hooks and the test hooks with nothing scheduled take the same
/// path in the temporal weighting stage: same output bits.
#[test]
fn product_tw_hooks_match_an_empty_hook_schedule() {
    let mut values: Vec<f64> = (0..997).map(|i| ((i * 13) % 29) as f64 * 0.5).collect();
    values[400] = -1.0; // out of range: the entry fallback reports it
    let hook_ctx = batch_hooks::StageHookContext::empty();
    let product = temporal_with(&values[..400], NoTwHooks).unwrap();
    let hooked = temporal_with(&values[..400], CtxTwHooks::new(&hook_ctx)).unwrap();
    assert_eq!(bits(&product), bits(&hooked));
    let product = temporal_with(&values, NoTwHooks).unwrap_err();
    let hooked = temporal_with(&values, CtxTwHooks::new(&hook_ctx)).unwrap_err();
    assert_eq!(format!("{product:?}"), format!("{hooked:?}"));
}

/// The product hooks and the test hooks with nothing scheduled take the same
/// path in both NL units: same output bits.
#[test]
fn product_nl_hooks_match_an_empty_hook_schedule() {
    const N: usize = 64;
    let core: Vec<f64> = (0..21 * N)
        .map(|i| ((i * 37) % 101) as f64 * 0.01)
        .collect();
    let b = nl_coeffs();
    let hook_ctx = batch_hooks::StageHookContext::empty();
    for init in [NlInit::Zero, NlInit::MosqitoTail] {
        for validated in [false, true] {
            let ctx = StageCtx {
                qualified: true,
                validated,
                avx2: false,
            };
            for band in 0..21 {
                let row = &core[band * N..(band + 1) * N];
                let (mut product, mut hooked) = (vec![0.0; N], vec![0.0; N]);
                nl_band(row, &mut product, &b, init, band, ctx, NoNlHooks).unwrap();
                let hooks = CtxNlHooks::new(&hook_ctx, band, 1, false);
                nl_band(row, &mut hooked, &b, init, band, ctx, hooks).unwrap();
                assert_eq!(bits(&product), bits(&hooked), "scalar band {band}");
            }
            #[cfg(target_arch = "x86_64")]
            if crate::simd::use_avx2() {
                for band in (0..20).step_by(4) {
                    let (mut product, mut hooked) = (vec![0.0; 4 * N], vec![0.0; 4 * N]);
                    let hooks = CtxNlHooks::new(&hook_ctx, band, 4, true);
                    // SAFETY: AVX2 and FMA checked just above.
                    unsafe {
                        avx2::nonlinear(&core, &mut product, N, band, b, init, ctx, NoNlHooks)
                            .unwrap();
                        avx2::nonlinear(&core, &mut hooked, N, band, b, init, ctx, hooks).unwrap();
                    }
                    assert_eq!(bits(&product), bits(&hooked), "avx2 group {band}");
                }
            }
        }
    }
}

#[test]
fn c4_temporal_lengths_bits_and_first_error_index() {
    for len in [0, 1, 2, 17, 100] {
        let values: Vec<_> = (0..len).map(|i| (i % 7) as f64 * 0.1).collect();
        assert_eq!(
            bits(&temporal(&values).unwrap()),
            bits(&crate::zwtv::temporal_weighting::temporal_weighting(
                &values
            ))
        );
        for index in 0..len {
            let mut bad = values.clone();
            bad[index] = f64::NAN;
            assert!(
                matches!(temporal(&bad),Err(SoneflowError::InternalNumericFault {stage:Stage::TemporalWeighting,index:i,band:None,..}) if i==index)
            );
        }
    }
}

#[test]
fn slope_faults_use_internal_time_then_band() {
    let mut total = [0.0; 12];
    let mut specific = [0.0; 720];
    total[8] = f64::NAN;
    specific[240 + 7] = f64::NAN;
    assert!(matches!(
        validate_slopes(&total, &specific),
        Err(SoneflowError::InternalNumericFault {
            index: 4,
            band: Some(7),
            ..
        })
    ));
    total[4] = f64::INFINITY;
    assert!(matches!(
        validate_slopes(&total, &specific),
        Err(SoneflowError::InternalNumericFault {
            index: 4,
            band: Some(7),
            ..
        })
    ));
    total[2] = f64::INFINITY;
    assert!(matches!(
        validate_slopes(&total, &specific),
        Err(SoneflowError::InternalNumericFault {
            index: 2,
            band: None,
            ..
        })
    ));
}

#[test]
fn checked_stages_preserve_normal_backend_bits() {
    let signal: Vec<_> = (0..4800)
        .map(|i| ((i % 71) as f64 - 35.0) * 0.0001)
        .collect();
    for mode in [ParMode::Sequential, ParMode::Rayon] {
        let expected =
            crate::zwtv::third_octave_levels::third_octave_levels_with_mode(&signal, mode);
        let actual = levels(&signal, mode).unwrap();
        assert_eq!(expected.1, actual.1);
        assert_eq!(bits(&expected.0), bits(&actual.0));
        let n = 120;
        let core: Vec<_> = (0..21 * n).map(|i| (i % 17) as f64 * 0.1).collect();
        for init in [NlInit::Zero, NlInit::MosqitoTail] {
            let expected = crate::zwtv::nonlinear_decay::nl_loudness_with(&core, n, mode, init);
            assert_eq!(
                bits(&expected),
                bits(&nonlinear(&core, n, mode, init).unwrap())
            );
        }
    }
    let total = [0.0, 0.1, 1.0, 0.0, 2.0, 0.5];
    assert_eq!(
        bits(&crate::zwtv::temporal_weighting::temporal_weighting(&total)),
        bits(&temporal(&total).unwrap())
    );
}

#[cfg(target_arch = "x86_64")]
struct MxcsrGuard(u32);
#[cfg(target_arch = "x86_64")]
impl MxcsrGuard {
    fn capture() -> Self {
        let mut v = 0u32;
        unsafe {
            std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut v, options(nostack, preserves_flags));
        }
        Self(v)
    }
    fn set(v: u32) {
        unsafe {
            std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &v, options(nostack));
        }
    }
}
#[cfg(target_arch = "x86_64")]
impl Drop for MxcsrGuard {
    fn drop(&mut self) {
        Self::set(self.0);
    }
}

#[test]
fn order1_mxcsr_qualification_predicate_exhaustive() {
    for rc in [0x0000u32, 0x2000, 0x4000, 0x6000] {
        for ftz_daz in [0x0000u32, 0x0040, 0x8000, 0x8040] {
            for masks in 0u32..64 {
                for sticky in [0x00u32, 0x01, 0x20, 0x3f] {
                    let word = rc | ftz_daz | (masks << 7) | sticky;
                    let expected =
                        rc == 0 && masks == 0x3f && (ftz_daz == 0x0000 || ftz_daz == 0x8040);
                    assert_eq!(
                        crate::numeric_guard::is_mxcsr_qualified(word),
                        expected,
                        "word={word:#06x}"
                    );
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn order1_batch_tol_dispatch_counters_and_precedence() {
    use crate::{FieldType, NlInit, ZwtvProcessor};
    use std::sync::Arc;

    let _restore = MxcsrGuard::capture();

    let sig: Vec<f64> = (0..4800)
        .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
        .collect();

    let has_avx2 = std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
    let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

    for &use_avx2 in backends {
        let expected_units = if use_avx2 { 7 } else { 28 };

        // 1. Qualified workers: RN(0,0) and RN(1,1), with and without sticky IE=1
        for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
            let sink = Arc::new(test_counters::Sink::default());
            let s_worker = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&s_worker)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
            assert!(res.is_ok(), "qualified worker {wcsr:#06x} failed: {res:?}");
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_qualified, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.avx2_unqualified, 0, "wcsr={wcsr:#06x}");
            } else {
                assert_eq!(c.scalar_qualified, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.scalar_unqualified, 0, "wcsr={wcsr:#06x}");
            }
        }

        // 2. Unqualified workers: RZ, RD, RU, FTZ=1/DAZ=0, FTZ=0/DAZ=1, OM unmasked (0x1b80)
        for wcsr in [0x7f80u32, 0x3f80, 0x5f80, 0x9f80, 0x1fc0, 0x1b80] {
            let sink = Arc::new(test_counters::Sink::default());
            let s_worker = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&s_worker)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
            assert!(
                res.is_ok(),
                "unqualified worker {wcsr:#06x} failed: {res:?}"
            );
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_qualified, 0, "wcsr={wcsr:#06x}");
                assert_eq!(c.avx2_unqualified, expected_units, "wcsr={wcsr:#06x}");
            } else {
                assert_eq!(c.scalar_qualified, 0, "wcsr={wcsr:#06x}");
                assert_eq!(c.scalar_unqualified, expected_units, "wcsr={wcsr:#06x}");
            }
        }

        // 3. Mixed worker pool (2 threads: thread 0 qualified 0x1f80, thread 1 unqualified 0x9f80).
        // Bind each worker thread to its own Sink, guarantee both workers execute units,
        // and assert worker 0 runs fast path exclusively while worker 1 runs R's checked loop.
        {
            let sink_q = Arc::new(test_counters::Sink::default());
            let sink_u = Arc::new(test_counters::Sink::default());
            let sq = Arc::clone(&sink_q);
            let su = Arc::clone(&sink_u);
            let tid = std::sync::atomic::AtomicUsize::new(0);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .start_handler(move |_| {
                    let id = tid.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if id == 0 {
                        MxcsrGuard::set(0x1f80);
                        test_counters::attach_thread(Some(Arc::clone(&sq)));
                    } else {
                        MxcsrGuard::set(0x9f80);
                        test_counters::attach_thread(Some(Arc::clone(&su)));
                    }
                })
                .build()
                .unwrap();

            let mut calls = 0usize;
            let (mut q_ran, mut u_ran) = (0usize, 0usize);
            for _ in 0..64 {
                let res = pool.install(|| levels_with_backend(&sig, ParMode::Rayon, use_avx2));
                assert!(res.is_ok(), "mixed worker pool failed: {res:?}");
                calls += 1;
                let cq = sink_q.snapshot();
                let cu = sink_u.snapshot();
                let (q_qual, q_unqual, q_fast, u_qual, u_unqual, u_fast) = if use_avx2 {
                    (
                        cq.avx2_qualified,
                        cq.avx2_unqualified,
                        cq.avx2_fast_blocks,
                        cu.avx2_qualified,
                        cu.avx2_unqualified,
                        cu.avx2_fast_blocks,
                    )
                } else {
                    (
                        cq.scalar_qualified,
                        cq.scalar_unqualified,
                        cq.scalar_fast_blocks,
                        cu.scalar_qualified,
                        cu.scalar_unqualified,
                        cu.scalar_fast_blocks,
                    )
                };
                assert_eq!(
                    q_unqual, 0,
                    "qualified worker (0x1f80) must never fall back to R (avx2={use_avx2})"
                );
                assert_eq!(
                    q_fast, q_qual,
                    "qualified worker (0x1f80) must execute fast path for every unit (avx2={use_avx2})"
                );
                assert_eq!(
                    u_qual, 0,
                    "unqualified worker (0x9f80) must never take fast path (avx2={use_avx2})"
                );
                assert_eq!(
                    u_fast, 0,
                    "unqualified worker (0x9f80) must execute R checked loop exclusively (avx2={use_avx2})"
                );
                assert_eq!(
                    q_qual + u_unqual,
                    expected_units * calls,
                    "total dispatched units mismatch (avx2={use_avx2})"
                );
                q_ran = q_qual;
                u_ran = u_unqual;
                if q_ran > 0 && u_ran > 0 {
                    break;
                }
            }
            assert!(
                q_ran > 0 && u_ran > 0,
                "both workers must execute units in mixed pool (avx2={use_avx2}, q_ran={q_ran}, u_ran={u_ran}, calls={calls})"
            );
        }
    }

    // 4. Existing R usage & caller environment rejections happen BEFORE worker dispatch (0 units)
    let sink = Arc::new(test_counters::Sink::default());
    let prev = test_counters::attach_thread(Some(Arc::clone(&sink)));
    MxcsrGuard::set(0x7f80); // caller non-RN
    let bad_rate = ZwtvProcessor::new().process_with(&sig, 44100.0, FieldType::Free, NlInit::Zero);
    assert!(matches!(
        bad_rate,
        Err(SoneflowError::UnsupportedSampleRate(_))
    ));
    let bad_short =
        ZwtvProcessor::new().process_with(&sig[..100], 48000.0, FieldType::Free, NlInit::Zero);
    assert!(matches!(
        bad_short,
        Err(SoneflowError::SignalTooShort { .. })
    ));
    let bad_env = ZwtvProcessor::new().process_with(&sig, 48000.0, FieldType::Free, NlInit::Zero);
    assert!(matches!(
        bad_env,
        Err(SoneflowError::InternalNumericFault {
            stage: Stage::Environment,
            ..
        })
    ));
    test_counters::attach_thread(prev);
    assert_eq!(sink.snapshot(), test_counters::TolDispatchCounts::default());
}

#[cfg(target_arch = "x86_64")]
#[test]
fn order2_batch_tol_fast_blocks_cold_replay_and_mxcsr_restore() {
    use std::sync::Arc;

    let _restore = MxcsrGuard::capture();
    let has_avx2 = std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
    let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

    let sig_norm: Vec<f64> = (0..4800)
        .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
        .collect();
    let mut sig_start = vec![0.01f64; 4800];
    sig_start[0] = f64::MAX;
    let mut sig_mid = vec![0.01f64; 4800];
    sig_mid[2400] = f64::MAX;
    let mut sig_end = vec![0.01f64; 4800];
    sig_end[4799] = f64::MAX;
    let sig_level = vec![1e155_f64; 4800];

    let mut sig_minnorm_tail = vec![0.0f64; 4800];
    sig_minnorm_tail[2400] = 1e200;
    for x in sig_minnorm_tail.iter_mut().skip(2401) {
        *x = f64::MIN_POSITIVE;
    }

    for &use_avx2 in backends {
        let expected_units = if use_avx2 { 7 } else { 28 };

        // 1. Normal signal under qualified workers: fast_blocks == units, cold_replays == 0
        for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| levels_with_backend(&sig_norm, ParMode::Rayon, use_avx2));
            assert!(res.is_ok(), "normal qualified {wcsr:#06x} failed: {res:?}");
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_fast_blocks, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.avx2_cold_replays, 0, "wcsr={wcsr:#06x}");
            } else {
                assert_eq!(c.scalar_fast_blocks, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.scalar_cold_replays, 0, "wcsr={wcsr:#06x}");
            }
        }

        // 2. Natural overflow at start/mid/end & level overflow: triggers cold replay
        for (tag, sig) in [
            ("start", sig_start.as_slice()),
            ("mid", sig_mid.as_slice()),
            ("end", sig_end.as_slice()),
            ("level", sig_level.as_slice()),
        ] {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(0x1f80);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| levels_with_backend(sig, ParMode::Rayon, use_avx2));
            assert!(
                matches!(
                    res,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        ..
                    })
                ),
                "{tag} expected Filter fault, got {res:?}"
            );
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_fast_blocks, expected_units, "{tag}");
                assert!(c.avx2_cold_replays > 0, "{tag} expected cold_replays > 0");
            } else {
                assert_eq!(c.scalar_fast_blocks, expected_units, "{tag}");
                assert!(c.scalar_cold_replays > 0, "{tag} expected cold_replays > 0");
            }
        }

        // 3. Min-normal tail fault: cold replay MUST restore MXCSR so UE (0x10) is NOT raised
        for wcsr in [0x1f80u32, 0x1f81, 0x9fc0, 0x9fc1] {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let (res, exit_csr) = pool.install(|| {
                MxcsrGuard::set(wcsr);
                let r = levels_with_backend(&sig_minnorm_tail, ParMode::Rayon, use_avx2);
                let after = MxcsrGuard::capture().0;
                (r, after)
            });
            assert!(
                matches!(
                    res,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        index: 2400,
                        band: Some(0),
                        ..
                    })
                ),
                "minnorm_tail {wcsr:#06x} got {res:?}"
            );
            assert_eq!(
                exit_csr & 0x10,
                0,
                "minnorm_tail {wcsr:#06x} leaked UE flag: exit_csr={exit_csr:#06x}"
            );
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_fast_blocks, expected_units);
                assert_eq!(c.avx2_cold_replays, expected_units);
            } else {
                assert_eq!(c.scalar_fast_blocks, expected_units);
                assert_eq!(c.scalar_cold_replays, expected_units);
            }
        }

        // 4. Unqualified worker: fast_blocks == 0, cold_replays == 0
        for wcsr in [0x7f80u32, 0x9f80, 0x1fc0, 0x1b80] {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| levels_with_backend(&sig_norm, ParMode::Rayon, use_avx2));
            assert!(res.is_ok());
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.avx2_unqualified, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.avx2_fast_blocks, 0, "wcsr={wcsr:#06x}");
                assert_eq!(c.avx2_cold_replays, 0, "wcsr={wcsr:#06x}");
            } else {
                assert_eq!(c.scalar_unqualified, expected_units, "wcsr={wcsr:#06x}");
                assert_eq!(c.scalar_fast_blocks, 0, "wcsr={wcsr:#06x}");
                assert_eq!(c.scalar_cold_replays, 0, "wcsr={wcsr:#06x}");
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn order5_batch_hooks_snapshots_due_cd_and_ie_case1() {
    use batch_hooks::BatchHookEvent;

    let _restore = MxcsrGuard::capture();
    let has_avx2 = std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
    let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };

    let calm: Vec<f64> = (0..4800)
        .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
        .collect();
    let hev = |sample, stage, band, field, raw_u64| BatchHookEvent {
        public_call_id: 0,
        sample,
        stage,
        band,
        field,
        raw_u64,
    };

    for &use_avx2 in backends {
        let pool1 = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x1f80))
            .build()
            .unwrap();

        // 1. Review counterexamples (s=25, s=0): fault must be at s, NOT natural 100
        let mut sig_nat100 = calm.clone();
        sig_nat100[100] = f64::MAX;
        for s_pos in [0usize, 25] {
            let res = pool1.install(|| {
                batch_hooks::install_thread_schedule(vec![hev(
                    s_pos,
                    1,
                    0,
                    0,
                    1e155_f64.to_bits(),
                )]);
                let r = levels_with_backend(&sig_nat100, ParMode::Rayon, use_avx2);
                batch_hooks::clear_thread_schedule();
                r
            });
            assert!(
                matches!(
                    res,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        index,
                        band: Some(0),
                        ..
                    }) if index == s_pos
                ),
                "counterexample s={s_pos} avx2={use_avx2}: got {res:?}"
            );
        }

        // 2. Multi-hook snapshot overwrite (hook1 at 25 valid, hook2 at 50 triggers 1e155 overflow)
        let res_m = pool1.install(|| {
            batch_hooks::install_thread_schedule(vec![
                hev(25, 1, 0, 0, 0.5_f64.to_bits()),
                hev(50, 1, 0, 0, 1e155_f64.to_bits()),
            ]);
            let r = levels_with_backend(&sig_nat100, ParMode::Rayon, use_avx2);
            batch_hooks::clear_thread_schedule();
            r
        });
        assert!(
            matches!(
                res_m,
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Filter,
                    index: 50,
                    band: Some(0),
                    ..
                })
            ),
            "multi-hook overwrite avx2={use_avx2}: got {res_m:?}"
        );

        // 3. Due rule c under DAZ (0x9fc0): neg min subnormal in sm[0] MUST be caught at s
        let pool_daz = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x9fc0))
            .build()
            .unwrap();
        for s_pos in [0usize, 25] {
            let res_daz = pool_daz.install(|| {
                batch_hooks::install_thread_schedule(vec![hev(
                    s_pos,
                    1,
                    0,
                    6,
                    0x8000_0000_0000_0001,
                )]);
                let r = levels_with_backend(&calm, ParMode::Rayon, use_avx2);
                batch_hooks::clear_thread_schedule();
                r
            });
            assert!(
                matches!(
                    res_daz,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        reason: crate::NumericFaultReason::NegativeDomain,
                        index,
                        band: Some(0),
                    }) if index == s_pos
                ),
                "DAZ negsub s={s_pos} avx2={use_avx2}: got {res_daz:?}"
            );
        }

        // 4. IE case 1: scalar R stops at sec 0 with IE=0; cold replay must restore MXCSR!
        if !use_avx2 {
            for init_csr in [0x1f80u32, 0x1f84, 0x9fc0, 0x9fc4] {
                let pool_ie = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .start_handler(move |_| MxcsrGuard::set(init_csr))
                    .build()
                    .unwrap();
                let mut sig_ie = vec![0.0f64; 4800];
                sig_ie[25] = f64::MAX;
                let (r, exit_csr) = pool_ie.install(|| {
                    MxcsrGuard::set(init_csr);
                    batch_hooks::install_thread_schedule(vec![hev(
                        25,
                        1,
                        0,
                        0,
                        f64::MAX.to_bits(),
                    )]);
                    let res = levels_with_backend(&sig_ie, ParMode::Rayon, false);
                    let after = MxcsrGuard::capture().0;
                    batch_hooks::clear_thread_schedule();
                    (res, after)
                });
                assert!(matches!(
                    r,
                    Err(SoneflowError::InternalNumericFault {
                        stage: Stage::Filter,
                        index: 25,
                        band: Some(0),
                        ..
                    })
                ));
                assert_eq!(
                    exit_csr & 1,
                    0,
                    "IE Case 1 leaked IE flag under init={init_csr:#06x}: exit={exit_csr:#06x}"
                );
            }
        }

        // 5. Due rule d (batch NL MosqitoTail due at t=0, cleared at t>=1; Zero not due unless hooked)
        let core_ok = vec![0.25f64; 21 * 8];
        let run_nl = |init: NlInit| {
            let b = nl_coeffs();
            let hook_ctx = batch_hooks::snapshot_stage(3, 8);
            let mut out = vec![0.0; 21 * 8];
            if use_avx2 {
                for g in 0..5usize {
                    let band = 4 * g;
                    unsafe {
                        avx2::nonlinear(
                            &core_ok,
                            &mut out[band * 8..(band + 4) * 8],
                            8,
                            band,
                            b,
                            init,
                            UNVALIDATED,
                            CtxNlHooks::new(&hook_ctx, band, 4, true),
                        )
                        .unwrap();
                    }
                }
                nl_band(
                    &core_ok[20 * 8..21 * 8],
                    &mut out[20 * 8..21 * 8],
                    &b,
                    init,
                    20,
                    UNVALIDATED,
                    CtxNlHooks::new(&hook_ctx, 20, 1, false),
                )
                .unwrap();
            } else {
                for band in 0..21usize {
                    nl_band(
                        &core_ok[band * 8..(band + 1) * 8],
                        &mut out[band * 8..(band + 1) * 8],
                        &b,
                        init,
                        band,
                        UNVALIDATED,
                        CtxNlHooks::new(&hook_ctx, band, 1, false),
                    )
                    .unwrap();
                }
            }
        };
        let (log_mosqito, log_zero, log_zero_hooked) = pool1.install(|| {
            batch_hooks::install_thread_schedule(Vec::new());
            batch_hooks::begin_due_log();
            run_nl(NlInit::MosqitoTail);
            let lm = batch_hooks::take_due_log();
            batch_hooks::begin_due_log();
            run_nl(NlInit::Zero);
            let lz = batch_hooks::take_due_log();
            batch_hooks::install_thread_schedule(vec![hev(3, 3, 0, 0, 0.5_f64.to_bits())]);
            batch_hooks::begin_due_log();
            run_nl(NlInit::Zero);
            let lzh = batch_hooks::take_due_log();
            batch_hooks::clear_thread_schedule();
            (lm, lz, lzh)
        });
        assert!(
            log_mosqito
                .iter()
                .filter(|r| r.stage == 3 && r.index == 0)
                .all(|r| r.due)
                && log_mosqito.iter().any(|r| r.stage == 3 && r.index == 0),
            "Rule d: MosqitoTail must set due_nl=true at t=0 (avx2={use_avx2})"
        );
        assert!(
            log_mosqito
                .iter()
                .filter(|r| r.stage == 3 && r.index > 0)
                .all(|r| !r.due),
            "Rule d: MosqitoTail due_nl must clear after t=0 (avx2={use_avx2})"
        );
        assert!(
            log_zero.iter().filter(|r| r.stage == 3).all(|r| !r.due),
            "NlInit::Zero without hooks must not set due_nl (avx2={use_avx2})"
        );
        assert!(
            log_zero_hooked
                .iter()
                .any(|r| r.stage == 3 && r.band == 0 && r.index == 3 && r.due)
                && log_zero_hooked
                    .iter()
                    .filter(|r| r.stage == 3 && !(r.band == 0 && r.index == 3))
                    .all(|r| !r.due),
            "Rule c: NL hook at t=3 band=0 must set due_nl ONLY at (band=0, t=3) (avx2={use_avx2})"
        );

        // 6. Due rule c for TW + invalid s = N rejection for TOL, NL, TW + NA stage rejection
        let tw_vals = vec![0.5f64; 8];
        let tw_log = pool1.install(|| {
            batch_hooks::install_thread_schedule(vec![hev(4, 5, 0, 0, 1.0_f64.to_bits())]);
            batch_hooks::begin_due_log();
            temporal(&tw_vals).unwrap();
            let l = batch_hooks::take_due_log();
            batch_hooks::clear_thread_schedule();
            l
        });
        assert!(tw_log.iter().any(|r| r.stage == 5 && r.index == 4 && r.due));
        assert!(tw_log
            .iter()
            .filter(|r| r.stage == 5 && r.index != 4)
            .all(|r| !r.due));

        // Invalid s == N MUST panic for TOL (4800), NL (8), TW (8), and NA stage (2)
        for (stage, bad_s) in [(1u32, 4800usize), (3, 8), (5, 8), (2, 0)] {
            let caught = pool1.install(|| {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    batch_hooks::install_thread_schedule(vec![hev(bad_s, stage, 0, 0, 0)]);
                    let _ = match stage {
                        1 | 2 => {
                            levels_with_backend(&calm, ParMode::Sequential, use_avx2).map(|_| ())
                        }
                        3 => nonlinear(&core_ok, 8, ParMode::Sequential, NlInit::Zero).map(|_| ()),
                        5 => temporal(&tw_vals).map(|_| ()),
                        _ => unreachable!(),
                    };
                }));
                batch_hooks::clear_thread_schedule();
                res
            });
            assert!(
                caught.is_err(),
                "stage={stage} s={bad_s} must panic as invalid/NA coordinate"
            );
        }

        // 7. Unqualified worker (0x9f80) MUST still execute batch TOL hooks!
        let pool_unq = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x9f80))
            .build()
            .unwrap();
        let res_unq = pool_unq.install(|| {
            batch_hooks::install_thread_schedule(vec![hev(25, 1, 0, 6, 0x8000_0000_0000_0001)]);
            let r = levels_with_backend(&calm, ParMode::Rayon, use_avx2);
            let w = batch_hooks::executed_writes();
            batch_hooks::clear_thread_schedule();
            (r, w)
        });
        assert_eq!(res_unq.1, 1);
        assert!(matches!(
            res_unq.0,
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::Filter,
                index: 25,
                band: Some(0),
                ..
            })
        ));
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn order5_batch_avx2_lane_isolation_tol_and_nl() {
    use batch_hooks::BatchHookEvent;

    if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
        return;
    }
    let _restore = MxcsrGuard::capture();
    let pool1 = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .start_handler(|_| MxcsrGuard::set(0x1f80))
        .build()
        .unwrap();

    let calm: Vec<f64> = (0..240)
        .map(|i| ((i as f64) * std::f64::consts::TAU * 1000.0 / 48000.0).sin() * 0.02)
        .collect();
    let hev = |sample, stage, band, field, raw_u64| BatchHookEvent {
        public_call_id: 0,
        sample,
        stage,
        band,
        field,
        raw_u64,
    };

    // 1. TOL AVX2 lane isolation: hooking one lane of group 0 (bands 0..4) must change ONLY
    //    that band's output; the 3 sibling lanes MUST remain bit-exact identical to unhooked.
    let (base_tol, n_tol) = pool1
        .install(|| levels_with_backend(&calm, ParMode::Sequential, true))
        .unwrap();
    for target_lane in 0..4usize {
        let (hooked_tol, n_h) = pool1
            .install(|| {
                batch_hooks::install_thread_schedule(vec![hev(
                    25,
                    1,
                    target_lane,
                    6,
                    1.5_f64.to_bits(),
                )]);
                let r = levels_with_backend(&calm, ParMode::Sequential, true);
                let w = batch_hooks::executed_writes();
                batch_hooks::clear_thread_schedule();
                assert_eq!(w, 1);
                r
            })
            .unwrap();
        assert_eq!(n_h, n_tol);
        for lane in 0..4usize {
            let b_slice = bits(&base_tol[lane * n_tol..(lane + 1) * n_tol]);
            let h_slice = bits(&hooked_tol[lane * n_tol..(lane + 1) * n_tol]);
            if lane == target_lane {
                assert_ne!(
                    h_slice, b_slice,
                    "TOL AVX2 hook on lane {target_lane} must affect target lane"
                );
            } else {
                assert_eq!(
                    h_slice, b_slice,
                    "TOL AVX2 hook on lane {target_lane} clobbered sibling lane {lane}"
                );
            }
        }
    }

    // 2. NL AVX2 lane isolation: hooking one lane of group 0 (bands 0..4) must change ONLY
    //    that band's output; the 3 sibling lanes MUST remain bit-exact identical to unhooked.
    let n_nl = 16usize;
    let core_ok: Vec<f64> = (0..21 * n_nl)
        .map(|i| 0.25 + ((i % 13) as f64) * 0.05)
        .collect();
    let b_coeffs = nl_coeffs();
    let run_nl_g0 = |hooks: Vec<BatchHookEvent>| -> Vec<f64> {
        pool1.install(|| {
            batch_hooks::install_thread_schedule(hooks);
            let hook_ctx = batch_hooks::snapshot_stage(3, n_nl);
            let mut out = vec![0.0; 4 * n_nl];
            unsafe {
                avx2::nonlinear(
                    &core_ok,
                    &mut out,
                    n_nl,
                    0,
                    b_coeffs,
                    NlInit::Zero,
                    UNVALIDATED,
                    CtxNlHooks::new(&hook_ctx, 0, 4, true),
                )
                .unwrap();
            }
            batch_hooks::clear_thread_schedule();
            out
        })
    };
    let base_nl = run_nl_g0(Vec::new());
    for target_lane in 0..4usize {
        let hooked_nl = run_nl_g0(vec![
            hev(3, 3, target_lane, 0, 5.0_f64.to_bits()),
            hev(3, 3, target_lane, 1, 2.0_f64.to_bits()),
        ]);
        for lane in 0..4usize {
            let b_slice = bits(&base_nl[lane * n_nl..(lane + 1) * n_nl]);
            let h_slice = bits(&hooked_nl[lane * n_nl..(lane + 1) * n_nl]);
            if lane == target_lane {
                assert_ne!(
                    h_slice, b_slice,
                    "NL AVX2 hook on lane {target_lane} must affect target lane"
                );
            } else {
                assert_eq!(
                    h_slice, b_slice,
                    "NL AVX2 hook on lane {target_lane} clobbered sibling lane {lane}"
                );
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn order6_batch_d0_nl_slopes_tw_dispatch_and_seed_entry() {
    use std::sync::Arc;

    let _restore = MxcsrGuard::capture();
    let has_avx2 = std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
    let backends: &[bool] = if has_avx2 { &[false, true] } else { &[false] };
    let n_time = 200usize;
    let core_ok: Vec<f64> = (0..21 * n_time)
        .map(|i| 0.25 + ((i % 13) as f64) * 0.05)
        .collect();

    // 1. Qualified 1-thread pool: NL, slopes, and TW MUST take D0 fast paths!
    for &use_avx2 in backends {
        for wcsr in [0x1f80u32, 0x9fc0] {
            let sink = Arc::new(test_counters::Sink::default());
            let sw = Arc::clone(&sink);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .start_handler(move |_| {
                    MxcsrGuard::set(wcsr);
                    test_counters::attach_thread(Some(Arc::clone(&sw)));
                })
                .build()
                .unwrap();
            let res = pool.install(|| -> Result<(), SoneflowError> {
                MxcsrGuard::set(wcsr);
                let nl = nonlinear_with_backend(
                    &core_ok,
                    n_time,
                    ParMode::Rayon,
                    NlInit::Zero,
                    true,
                    use_avx2,
                )?;
                let mut loud = Vec::new();
                let mut spec = Vec::new();
                let mut unq =
                    crate::zwtv::slopes::slopes_into_d0(&nl, n_time, &mut loud, &mut spec);
                validate_slopes_chunks(&loud, &spec, &mut unq)?;
                let _ = temporal(&loud)?;
                Ok(())
            });
            assert!(res.is_ok(), "avx2={use_avx2} wcsr={wcsr:#06x}: {res:?}");
            let c = sink.snapshot();
            if use_avx2 {
                assert_eq!(c.nl_avx2_qualified, 5, "avx2 NL 5 groups");
                assert_eq!(c.nl_avx2_unqualified, 0);
                assert_eq!(c.nl_avx2_fast_frames, 5 * n_time);
                assert_eq!(c.nl_scalar_qualified, 1, "avx2 NL scalar tail band 20");
                assert_eq!(c.nl_scalar_fast_frames, n_time);
            } else {
                assert_eq!(c.nl_scalar_qualified, 21, "scalar NL 21 bands");
                assert_eq!(c.nl_scalar_unqualified, 0);
                assert_eq!(c.nl_scalar_fast_frames, 21 * n_time);
            }
            assert_eq!(
                c.slopes_qualified, 50,
                "200 frames / OUT_DECIM(4) = 50 tasks"
            );
            assert_eq!(c.slopes_unqualified, 0);
            assert_eq!(c.slopes_unqual_validated, 0);
            assert_eq!(c.tw_qualified, 1);
            assert_eq!(c.tw_unqualified, 0);
            assert_eq!(c.tw_fast_frames, n_time);
        }
    }

    // 2. Unqualified worker (0x9f80 FTZ=1,DAZ=0):
    //    NL tasks, slopes tasks, and TW MUST fall back to R checked / R validate_slopes_chunks!
    for &use_avx2 in backends {
        let sink = Arc::new(test_counters::Sink::default());
        let sw = Arc::clone(&sink);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(move |_| {
                MxcsrGuard::set(0x9f80);
                test_counters::attach_thread(Some(Arc::clone(&sw)));
            })
            .build()
            .unwrap();
        let res = pool.install(|| -> Result<(), SoneflowError> {
            let nl = nonlinear_with_backend(
                &core_ok,
                n_time,
                ParMode::Rayon,
                NlInit::Zero,
                true,
                use_avx2,
            )?;
            let mut loud = Vec::new();
            let mut spec = Vec::new();
            let mut unq = crate::zwtv::slopes::slopes_into_d0(&nl, n_time, &mut loud, &mut spec);
            validate_slopes_chunks(&loud, &spec, &mut unq)?;
            let _ = temporal(&loud)?;
            Ok(())
        });
        assert!(res.is_ok());
        let c = sink.snapshot();
        if use_avx2 {
            assert_eq!(c.nl_avx2_qualified, 0);
            assert_eq!(c.nl_avx2_unqualified, 5);
            assert_eq!(c.nl_avx2_fast_frames, 0);
            assert_eq!(c.nl_scalar_unqualified, 1);
            assert_eq!(c.nl_scalar_fast_frames, 0);
        } else {
            assert_eq!(c.nl_scalar_qualified, 0);
            assert_eq!(c.nl_scalar_unqualified, 21);
            assert_eq!(c.nl_scalar_fast_frames, 0);
        }
        assert_eq!(c.slopes_qualified, 0);
        assert_eq!(c.slopes_unqualified, 50);
        assert_eq!(c.slopes_unqual_validated, 50);
        assert_eq!(c.tw_qualified, 0);
        assert_eq!(c.tw_unqualified, 1);
        assert_eq!(c.tw_fast_frames, 0);
    }

    // 3. MosqitoTail 13*eta seed negative entry check under RN(0,0): MUST catch NegativeDomain at frame 0!
    for &use_avx2 in backends {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x1f80))
            .build()
            .unwrap();
        let mut core = vec![0.0f64; 21 * 4];
        core[3] = f64::from_bits(13);
        let res = pool.install(|| {
            nonlinear_with_backend(
                &core,
                4,
                ParMode::Rayon,
                NlInit::MosqitoTail,
                true,
                use_avx2,
            )
        });
        assert!(
            matches!(
                res,
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Nonlinear,
                    reason: crate::NumericFaultReason::NegativeDomain,
                    index: 0,
                    band: Some(0),
                })
            ),
            "MosqitoTail 13*eta seed must fail NegativeDomain at frame 0 (avx2={use_avx2}): got {res:?}"
        );
    }

    // 4. validate_slopes_chunks preserves exact R (frame, band) ordering (total band = usize::MAX)
    {
        let mut total = [0.0; 12];
        let mut specific = [0.0; 720];
        total[8] = f64::NAN;
        specific[240 + 7] = f64::NAN;
        assert!(matches!(
            validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::Slope,
                index: 4,
                band: Some(7),
                ..
            })
        ));
        total[4] = f64::INFINITY;
        assert!(matches!(
            validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::Slope,
                index: 4,
                band: Some(7),
                ..
            })
        ));
        total[2] = f64::INFINITY;
        assert!(matches!(
            validate_slopes_chunks(&total, &specific, &mut [2, 1, 0]),
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::Slope,
                index: 2,
                band: None,
                ..
            })
        ));
    }

    // 5. DAZ (0x9fc0): -0.0 accepted as legal; -1*eta rejected as NegativeDomain for NL & TW
    {
        let pool_daz = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .start_handler(|_| MxcsrGuard::set(0x9fc0))
            .build()
            .unwrap();
        let neg_zero_vals = vec![f64::from_bits(0x8000_0000_0000_0000); 4];
        let mut neg_sub_vals = vec![0.0f64; 4];
        neg_sub_vals[2] = f64::from_bits(0x8000_0000_0000_0001);
        assert!(pool_daz.install(|| temporal(&neg_zero_vals)).is_ok());
        assert!(matches!(
            pool_daz.install(|| temporal(&neg_sub_vals)),
            Err(SoneflowError::InternalNumericFault {
                stage: Stage::TemporalWeighting,
                reason: crate::NumericFaultReason::NegativeDomain,
                index: 2,
                band: None,
            })
        ));
        for &use_avx2 in backends {
            let core_nz = vec![f64::from_bits(0x8000_0000_0000_0000); 21 * 4];
            assert!(pool_daz
                .install(|| nonlinear_with_backend(
                    &core_nz,
                    4,
                    ParMode::Rayon,
                    NlInit::Zero,
                    false,
                    use_avx2
                ))
                .is_ok());
            let mut core_ns = vec![0.0f64; 21 * 4];
            core_ns[20 * 4 + 1] = f64::from_bits(0x8000_0000_0000_0001);
            assert!(matches!(
                pool_daz.install(|| nonlinear_with_backend(
                    &core_ns,
                    4,
                    ParMode::Rayon,
                    NlInit::Zero,
                    false,
                    use_avx2
                )),
                Err(SoneflowError::InternalNumericFault {
                    stage: Stage::Nonlinear,
                    reason: crate::NumericFaultReason::NegativeDomain,
                    index: 0,
                    band: Some(20),
                })
            ));
        }
    }
}
