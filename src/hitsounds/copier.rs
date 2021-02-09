use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use libosu::prelude::*;

#[derive(Debug, StructOpt)]
pub struct CopyHitsoundOpts {
    /// The map to copy hitsounds from.
    pub src: PathBuf,

    /// The map to copy hitsounds to.
    pub dsts: Vec<PathBuf>,
}

pub fn copy_hitsounds(opts: CopyHitsoundOpts) -> Result<()> {
    let file = File::open(&opts.src)?;
    let src_beatmap = Beatmap::parse(file)?;

    let hitsound_data = collect_hitsounds(&src_beatmap)?;
    for hit in hitsound_data.hits.iter() {
        println!("{:?}", hit);
    }

    for dst in opts.dsts.iter() {
        let mut dst_beatmap = {
            let file = File::open(dst)?;
            Beatmap::parse(file)?
        };

        apply_hitsounds(&hitsound_data, &mut dst_beatmap)?;
        {
            let file = File::create(dst)?;
            dst_beatmap.write(file)?;
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct HitsoundData {
    hits: Vec<HitsoundInfo>,
}

/// Hitsound data for a single instant
#[derive(Debug)]
pub struct HitsoundInfo {
    pub time: TimestampMillis,
    pub vol: u16,
    pub additions: Additions,
    pub sample_set: SampleSet,
    pub addition_set: SampleSet,
}

/// Returns all the information extracted from the beatmap that can be used to copy hitsounds to a
/// different beatmap.
fn collect_hitsounds(beatmap: &Beatmap) -> Result<HitsoundData> {
    let mut hits = Vec::new();

    for (ho, tp) in beatmap.double_iter() {
        let start_time = ho.start_time;

        // if this hitsound doesn't have a sample set, default to the timing point's
        let sample_set = if let SampleSet::None = ho.sample_info.sample_set {
            tp.sample_set
        } else {
            ho.sample_info.sample_set
        };

        // if this hitsound doesn't have an addition set, then use the sample set
        let addition_set = if let SampleSet::None = ho.sample_info.addition_set {
            sample_set
        } else {
            ho.sample_info.addition_set
        };

        match &ho.kind {
            HitObjectKind::Circle => {
                hits.push(HitsoundInfo {
                    time: start_time,
                    vol: tp.volume,
                    sample_set,
                    addition_set,
                    additions: ho.additions,
                });
            }

            HitObjectKind::Slider(info) => {
                let duration = beatmap.get_slider_duration(ho).unwrap();
                let mut time = ho.start_time.0 as f64;

                // add a hitsound for each slider repeat (called "edge")
                for (additions, (normal_set, addition_set)) in
                    info.edge_additions.iter().zip(info.edge_samplesets.iter())
                {
                    let edge_sample_set = if let SampleSet::None = normal_set {
                        // default to the hit object's sample set
                        sample_set
                    } else {
                        *normal_set
                    };
                    let edge_addition_set = if let SampleSet::None = addition_set {
                        // default to the edge sample set
                        edge_sample_set
                    } else {
                        *addition_set
                    };
                    hits.push(HitsoundInfo {
                        time: TimestampMillis(time as i32),
                        vol: tp.volume,
                        sample_set: edge_sample_set,
                        addition_set: edge_addition_set,
                        additions: *additions,
                    });
                    time += duration;
                }
            }

            HitObjectKind::Spinner(info) => {
                // the hitsound for a spinner is at the end only
                hits.push(HitsoundInfo {
                    time: info.end_time,
                    vol: tp.volume,
                    sample_set,
                    addition_set,
                    additions: ho.additions,
                });
            }
        }
    }

    Ok(HitsoundData { hits })
}

/// Given a set of hitsound data, and a beatmap, applies the hitsound data to the beatmap.
fn apply_hitsounds(hitsound_data: &HitsoundData, beatmap: &mut Beatmap) -> Result<()> {
    // this is a list of (hitsound index, hitobject index)
    let mut circle_map = Vec::new();
    // this is a list of (hitsound index, hitobject index, slider index)
    let mut slider_map = Vec::new();

    // iterate over all the hitobjects in the map
    let mut iter = beatmap.hit_objects.iter().enumerate().peekable();
    'outer: for (hs_idx, hit) in hitsound_data.hits.iter().enumerate() {
        // get the next hitobject
        let (ho_idx, ho) = loop {
            let (ho_idx, ho) = match iter.peek() {
                Some((ho_idx, ho)) => (*ho_idx, ho),
                None => break 'outer,
            };

            let ho_end_time = beatmap.get_hitobject_end_time(ho);
            if ho_end_time >= hit.time {
                break (ho_idx, ho);
            }

            iter.next();
        };

        if ho.start_time == hit.time {
            if let HitObjectKind::Circle = ho.kind {
                circle_map.push((hs_idx, ho_idx));
            } else if let HitObjectKind::Slider { .. } = ho.kind {
                circle_map.push((hs_idx, ho_idx));
            }
        } else if ho.start_time < hit.time {
            if let HitObjectKind::Spinner(SpinnerInfo { end_time }) = ho.kind {
                if end_time == hit.time {
                    circle_map.push((hs_idx, ho_idx));
                }
            } else if let HitObjectKind::Slider(SliderInfo { num_repeats, .. }) = ho.kind {
                let time_diff = (hit.time.0 - ho.start_time.0) as f64;
                let duration = beatmap.get_slider_duration(ho).unwrap();
                let num_repeats_approx = time_diff / duration;
                let num_repeats_rounded = num_repeats_approx.round();
                if num_repeats_rounded as u32 > num_repeats {
                    continue;
                }
                let percent_diff = (num_repeats_rounded - num_repeats_approx).abs();
                if percent_diff < 0.05 {
                    let num_repeats = num_repeats_rounded as usize;
                    slider_map.push((hs_idx, ho_idx, num_repeats));
                }
            }
        }
    }

    // apply hitsounds to the hitobjects that have hitosunds on the start
    for (hs_idx, ho_idx) in circle_map {
        let hitsound = hitsound_data.hits.get(hs_idx).unwrap();
        let mut hit_object = beatmap.hit_objects.get_mut(ho_idx).unwrap();

        hit_object.additions = hitsound.additions;
        hit_object.sample_info.sample_set = hitsound.sample_set;
        hit_object.sample_info.addition_set = hitsound.addition_set;
    }

    // apply hitsounds to sliders
    for (hs_idx, ho_idx, e_idx) in slider_map {
        let hitsound: &HitsoundInfo = hitsound_data.hits.get(hs_idx).unwrap();
        let hit_object: &mut HitObject = beatmap.hit_objects.get_mut(ho_idx).unwrap();
        if let HitObjectKind::Slider(SliderInfo {
            ref mut edge_additions,
            ref mut edge_samplesets,
            ..
        }) = hit_object.kind
        {
            while edge_additions.len() <= e_idx {
                edge_additions.push(Additions::empty());
            }
            edge_additions[e_idx] = hitsound.additions;

            while edge_samplesets.len() <= e_idx {
                edge_samplesets.push((SampleSet::None, SampleSet::None));
            }
            edge_samplesets[e_idx] = (hitsound.sample_set, hitsound.addition_set);
        }
    }

    Ok(())
}

/// Erases all hitsounds from a map.
fn reset_hitsounds(beatmap: &mut Beatmap) {
    for ho in beatmap.hit_objects.iter_mut() {
        ho.additions = Additions::empty();
        ho.sample_info = SampleInfo::default();

        if let HitObjectKind::Slider(info) = &mut ho.kind {
            info.edge_additions.iter_mut().for_each(|addition| {
                *addition = Additions::empty();
            });

            info.edge_samplesets
                .iter_mut()
                .for_each(|(sample_set, addition_set)| {
                    *sample_set = SampleSet::None;
                    *addition_set = SampleSet::None;
                });
        }
    }
}
