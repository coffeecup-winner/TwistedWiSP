//! Public Rust API regression tests; no audio device or GUI fixtures required.
use std::path::Path;
use twisted_wisp::OfflineSession;

#[test]
fn phasor_output_remains_valid_after_session_destruction() {
    let mut session = OfflineSession::new(
        44100,
        2,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
    )
    .unwrap();
    let name = session
        .load_flow_text(
            "[flow]\nname='phasor_test'\n\
             [[flow.nodes]]\ntext='phasor'\n\
             [[flow.nodes]]\ntext='= 440'\n\
             [[flow.nodes]]\ntext='out'\n\
             [[flow.edges]]\nfrom=1\nto=0\noutput_index=0\ninput_index=0\n\
             [[flow.edges]]\nfrom=0\nto=2\noutput_index=0\ninput_index=0\n",
        )
        .unwrap();
    let mut processor = session.compile(&name).unwrap();
    drop(session);

    let mut frame = [0.; 2];
    processor.process_one_checked(&mut frame).unwrap();
    // Preserve the original Python regression's expected first stereo sample.
    assert_eq!(frame, [0.009977324_f32; 2]);

    let mut phase = frame[0];
    let mut frames = [0.; 32];
    processor.process_all_checked(&mut frames).unwrap();
    for frame in frames.chunks_exact(2) {
        phase = (phase + 440_f32 / 44100_f32) % 1.;
        assert_eq!(frame, &[phase; 2]);
    }
}

#[test]
fn checked_processing_rejects_invalid_frames_without_writing_output() {
    let mut session = OfflineSession::new(
        44100,
        3,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
    )
    .unwrap();
    let name = session
        .load_flow_text("[flow]\nname='empty'\nnodes=[]\nedges=[]\n")
        .unwrap();
    let mut processor = session.compile(&name).unwrap();
    drop(session);
    assert_eq!(processor.output_channels(), 3);

    let mut short = [7.; 2];
    assert!(processor.process_one_checked(&mut short).is_err());
    assert!(processor.process_all_checked(&mut short).is_err());
    assert_eq!(short, [7.; 2]);

    let mut incomplete = [7.; 4];
    assert!(processor.process_all_checked(&mut incomplete).is_err());
    assert_eq!(incomplete, [7.; 4]);
    processor.process_all_checked(&mut []).unwrap();

    let mut frame = [7.; 3];
    processor.process_one_checked(&mut frame).unwrap();
    assert_eq!(frame, [0.; 3]);
    let mut frames = [7.; 6];
    processor.process_all_checked(&mut frames).unwrap();
    assert_eq!(frames, [0.; 6]);
}
