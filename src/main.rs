use gst::prelude::*;
use gst_rtp::RTPBuffer;
use anyhow::Error;
use gst::ClockTime;

// Working version
// GST_DEBUG=2 gst-launch-1.0 rtspsrc location=rtsp://admin:spot2330@10.0.0.7:554/ name=rtspsrc latency=10 \
// rtspsrc. ! rtph265depay ! tee name=t t. ! queue ! rtph265pay config-interval=-1 ! udpsink name=udpsink host=127.0.0.1 port=56001

// Inside main
fn main() {
    gst::init().unwrap();

    let pipeline_str = "rtspsrc location=rtsp://admin:spot2330@10.0.0.249:554/ name=rtspsrc latency=100 \
     rtspsrc. ! rtph265depay ! tee name=t \
        t. ! queue ! h265parse ! rtph265pay name=rtph265pay config-interval=-1 ! udpsink name=udpsink host=127.0.0.1 port=56001";

    // let pipeline_str = "rtspsrc location=rtsp://admin:spot2330@10.0.0.249:554/ name=rtspsrc latency=100 \
    //  rtspsrc. ! rtph264depay ! tee name=t \
    //     t. ! queue ! h264parse ! rtph264pay name=rtph265pay config-interval=-1 ! udpsink name=udpsink host=127.0.0.1 port=56001";


    let pipe_elem = gst::parse_launch(pipeline_str).unwrap();

    let pipeline = pipe_elem.clone().downcast::<gst::Pipeline>().unwrap();

    let rtph265pay = pipeline.by_name("rtph265pay").unwrap();

    let rtppay_src_pad = rtph265pay.static_pad("src").unwrap();

    let rtspsrc = pipeline.by_name("rtspsrc").unwrap();

    let bin_ref = pipeline.clone();

    rtspsrc.connect_pad_added(move |_, src_pad| {
        match src_pad.current_caps() {
            Some(caps) => {
                let new_pad_struct = caps.structure(0).expect("Failed to get first structure of caps for audio");
                for i in 0..new_pad_struct.n_fields() {
                    match new_pad_struct.nth_field_name(i).unwrap().as_str() {
                        "media" => {
                            let media_type = new_pad_struct.value("media").unwrap();
                            let field_value = media_type.get::<&str>().unwrap();
                            println!("field_value={}", field_value);
                            if field_value == "video" {
                                bin_ref.debug_to_dot_file(gst::DebugGraphDetails::all(), "PLAYING");
                                rtppay_src_pad.add_probe(gst::PadProbeType::BUFFER, |pad, probe_info| {
                                    println!("adding probe for rtppay");
                                    if let Some(probe_data) = probe_info.data.as_mut() {
                                        if let gst::PadProbeData::Buffer(ref mut buffer) = probe_data {
                                            let size = buffer.size();
                                            match buffer.pts() {
                                                Some(pts) => {
                                                    println!("ptstime={}", pts.seconds())
                                                },
                                                None => {
                                                    println!("No PTS, cannot get bandwidth")
                                                }
                                            }

                                            let b = buffer.get_mut().unwrap();
                                            let mut rtp_buffer = RTPBuffer::from_buffer_writable(b).unwrap();

                                            let pts = rtp_buffer.buffer().pts().unwrap();
                                            // Convert the PTS to bytes
                                            let pts_bytes = pts.to_be_bytes();
                                            let extension_data = &pts_bytes[..];

                                            let appbits = 5; // Custom application-specific bits
                                            let id = 1; // Custom extension ID
                                            let result = rtp_buffer.add_extension_twobytes_header(appbits, id, extension_data);

                                            if let Err(e) = result {
                                                eprintln!("Failed to add RTP header extension: {:?}", e);
                                            }
                                        }
                                    }
                                    gst::PadProbeReturn::Ok
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    });


    let pipeline = pipe_elem.clone().downcast::<gst::Pipeline>().unwrap();
    let udpsink = pipeline.by_name("udpsink").unwrap();

    let udpsink_sink_pad = udpsink.static_pad("sink").unwrap();
    udpsink_sink_pad.add_probe(gst::PadProbeType::BUFFER, |pad, probe_info| {
        println!("GURU: adding pad probe to udpsink");
        if let Some(probe_data) = probe_info.data.as_ref() {
            if let gst::PadProbeData::Buffer(buffer) = probe_data {
                let rtp_buffer = RTPBuffer::from_buffer_readable(buffer).unwrap();
                // Check for RTP extension header
                if let Some((appbits, extension_data)) = rtp_buffer.extension_twobytes_header(1, 0) { //extension_twobytes_header(1, 0) {
                    println!("RTP Extension present:");
                    println!("App bits: {}", appbits);
                    println!("Extension data: {:?}", extension_data);

                    // Convert the extension data back to PTS
                    if extension_data.len() != 0 {
                        let mut pts_bytes = [0u8; 8];
                        pts_bytes[..4].copy_from_slice(&extension_data[..4]);  // Copy the first 4 bytes
                        let pts = u64::from_be_bytes(pts_bytes);
                        //let pts = u64::from_be_bytes(extension_data.try_into().unwrap());
                        println!("Extracted PTS from RTP extension: {}", pts);
                    }
                } else {
                    println!("No RTP Extension found");
                }
                match rtp_buffer.buffer().pts() {
                    Some(pts) => {
                        println!("udpsink buffer.pts={}", pts.seconds());
                    },
                    None => {
                        println!("No PTS, cannot get bandwidth");
                    }
                }
            }
        }
        gst::PadProbeReturn::Ok
    }).unwrap();

    // Start the pipeline and run the main loop
    let bus = pipeline.bus().unwrap();
    let pipeline_weak = pipeline.downgrade();
    let _watch = bus.add_watch_local(move |_, msg| {
        if let Some(pipeline) = pipeline_weak.upgrade() {
            match msg.view() {
                gst::MessageView::Element(msg) => {
                    println!("msg.name()={}", msg.src().unwrap().name());
                },
                _ => (),
            }
        }
        glib::ControlFlow::Continue
    }).unwrap();


    pipeline.set_state(gst::State::Playing).unwrap();

    let main_loop = glib::MainLoop::new(None, false);
    main_loop.run();
}
