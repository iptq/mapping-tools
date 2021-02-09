use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use libosu::prelude::*;

#[derive(Debug, StructOpt)]
pub struct CopyHitsoundOpts {
    /// The path of the map to copy hitsounds from.
    pub src: PathBuf,

    /// The paths of maps to copy hitsounds to.
    pub dsts: Vec<PathBuf>,

    /// Temporal leniency, the number of milliseconds apart two objects can be apart
    #[structopt(short = "l", long = "leniency", default_value = "2")]
    pub leniency: u32,
}

pub fn copy_hitsounds(opts: CopyHitsoundOpts) -> Result<()> {
    let file = File::open(&opts.src)?;
    let src_beatmap = Beatmap::parse(file)?;

    let hitsound_data = collect_hitsounds(&src_beatmap, &opts)?;
    for hit in hitsound_data.hits.iter() {
        debug!("collected_hit: {:?}", hit);
    }

    for dst in opts.dsts.iter() {
        let mut dst_beatmap = {
            let file = File::open(dst)?;
            Beatmap::parse(file)?
        };

        apply_hitsounds(&hitsound_data, &mut dst_beatmap, &opts)?;
        {
            let file = File::create(dst)?;
            dst_beatmap.write(file)?;
        }
    }

    Ok(())
}

/// All information about the hitsounds in a single file, that can be used to copy to another
/// beatmap without more information from the original beatmap.
#[derive(Debug)]
pub struct HitsoundData {
    hits: Vec<HitsoundInfo>,
    vols: Vec<(Millis, u16)>,
}

/// Hitsound data for a single instant
#[derive(Debug)]
pub struct HitsoundInfo {
    pub time: f64,
    pub additions: Additions,
    pub sample_info: SampleInfo,
}

/// Returns all the information extracted from the beatmap that can be used to copy hitsounds to a
/// different beatmap.
fn collect_hitsounds(beatmap: &Beatmap, _opts: &CopyHitsoundOpts) -> Result<HitsoundData> {
    let mut hits = Vec::new();
    let hit_times = get_hit_times(beatmap)?;

    let mut tp_idx = 0;
    for (hit_time, ho_idx, repeat_idx) in hit_times {
        // find out the correct timing point corresponding to this time
        // TODO: could possibly just track this with a variable?
        let tp = match (
            beatmap.timing_points.get(tp_idx),
            beatmap.timing_points.get(tp_idx + 1),
        ) {
            // next one if we're there
            (_, Some(tp)) if Millis::from_seconds(hit_time) >= tp.time => {
                tp_idx += 1; // bump up the timing point index
                tp
            }
            (Some(tp), _) => tp, // last timing point
            _ => break,          // we're out of timing points?
        };

        // get the hitobject
        let ho = match beatmap.hit_objects.get(ho_idx) {
            Some(ho) => ho,
            None => continue,
        };

        // check for the additions
        // note: if it's a slider, get addition info from the edge_additions
        let additions = if let HitObjectKind::Slider(info) = &ho.kind {
            if let Some(repeat_idx) = repeat_idx {
                info.edge_additions[repeat_idx]
            } else {
                ho.additions
            }
        } else {
            ho.additions
        };

        // get the sample sets info
        // note: if it's a slider, the edge_samplesets overrides
        let mut sample_info = if let HitObjectKind::Slider(info) = &ho.kind {
            let mut sample_info = ho.sample_info.clone();
            if let Some(repeat_idx) = repeat_idx {
                let samplesets = info.edge_samplesets[repeat_idx];
                sample_info.sample_set = samplesets.0;
                sample_info.addition_set = samplesets.1;
            }
            sample_info
        } else {
            ho.sample_info.clone()
        };

        // default the sample set to the timing point's sample set
        if let SampleSet::None = sample_info.sample_set {
            sample_info.sample_set = tp.sample_set;
        }

        // default the additions set to the normal sample set
        if let SampleSet::None = sample_info.addition_set {
            sample_info.addition_set = sample_info.sample_set;
        }

        hits.push(HitsoundInfo {
            time: hit_time,
            additions,
            sample_info,
        });
    }
    hits.sort_by_key(|h| NotNan::new(h.time).unwrap());

    let mut vols = Vec::new();

    // collect all the volumes from the timing points
    let mut last_vol = None;
    for tp in beatmap.timing_points.iter() {
        let should_push = if let Some(last_vol) = last_vol {
            last_vol != tp.volume
        } else {
            true
        };

        if should_push {
            vols.push((tp.time, tp.volume));
        }

        last_vol = Some(tp.volume);
    }
    vols.sort_by_key(|(t, _)| *t);

    Ok(HitsoundData { hits, vols })
}

/// Given a set of hitsound data, and a beatmap, applies the hitsound data to the beatmap.
fn apply_hitsounds(
    hitsound_data: &HitsoundData,
    beatmap: &mut Beatmap,
    opts: &CopyHitsoundOpts,
) -> Result<()> {
    // doesn't hurt to make sure that these lists are sorted
    beatmap.hit_objects.sort_by_key(|ho| ho.start_time);
    beatmap.timing_points.sort_by_key(|tp| tp.time);

    let leniency = Millis(opts.leniency as i32).as_seconds();

    let hit_times = get_hit_times(&beatmap)?;
    for (hit_time, ho_idx, repeat_idx) in hit_times {
        // determine the hit using binary search over the collected data
        let hit = match binary_search_for(hit_time, &hitsound_data.hits, leniency) {
            Some(hit) => hit,
            None => {
                info!("did not find hitsound for time={}", hit_time);
                continue;
            }
        };

        if (hit_time - hit.time).abs() > leniency {
            continue;
        }

        // get the hitobject
        let ho = match beatmap.hit_objects.get_mut(ho_idx) {
            Some(ho) => ho,
            None => continue,
        };

        trace!(
            "ho_idx: {}, ho_time: {}, hit_time: {}, hit.time: {}, diff: {}, repeat: {:?}",
            ho_idx,
            ho.start_time.0,
            hit_time,
            hit.time,
            (hit_time - hit.time).abs(),
            repeat_idx
        );
        trace!("hit: {:?}", hit);

        if let Some(repeat_idx) = repeat_idx {
            if let HitObjectKind::Slider(info) = &mut ho.kind {
                // make sure it has that # of repeats
                info.edge_samplesets.resize(
                    info.num_repeats as usize + 1,
                    (SampleSet::None, SampleSet::None),
                );
                info.edge_additions
                    .resize(info.num_repeats as usize + 1, Additions::empty());

                info.edge_samplesets[repeat_idx] =
                    (hit.sample_info.sample_set, hit.sample_info.addition_set);
                info.edge_additions[repeat_idx] = hit.additions;
                trace!(
                    "slider @ {} [repeat={}] (time={}) .edge_sets={:?}, .edge_additions={:?}",
                    ho.start_time.0,
                    repeat_idx,
                    hit_time,
                    (hit.sample_info.sample_set, hit.sample_info.addition_set),
                    hit.additions
                );
            }
        } else {
            ho.sample_info = hit.sample_info.clone();
            ho.additions = hit.additions.clone();
        }
    }

    // apply the volumes to the timing points
    // loop over volume ranges
    for w in hitsound_data.vols.windows(2) {
        let (first_vol_t, first_vol) = w[0];
        let (second_vol_t, second_vol) = w[1];
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

fn binary_search_for(
    hit_time: f64,
    hit_times: &[HitsoundInfo],
    leniency: f64,
) -> Option<&HitsoundInfo> {
    trace!("binary searching for {}", hit_time);
    let mut lo = 0;
    let mut hi = hit_times.len() - 1;

    while hi >= lo {
        let mid = (lo + hi) / 2;
        let k = &hit_times[mid];
        let time = k.time;
        trace!("lo={} hi={} mid={} mid_t={} mid={:?}", lo, hi, mid, time, k);

        if (time - hit_time).abs() < leniency {
            return Some(k);
        } else if time < hit_time {
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }

    None
}

/// Collect a list of EVERY possible time a hitsound could be played
///
/// This includes hitcircles, and every repeat / tail of sliders. The return value is
/// (timestamp in seconds, index of hitobject, index of repeat (if slider))
///
/// Notably, this assumes that the hit_objects is sorted, since it refers to hit_objects by index
fn get_hit_times(beatmap: &Beatmap) -> Result<Vec<(f64, usize, Option<usize>)>> {
    let mut hit_times = Vec::new();

    for (idx, ho) in beatmap.hit_objects.iter().enumerate() {
        use HitObjectKind::*;
        match &ho.kind {
            Circle => hit_times.push((ho.start_time.as_seconds(), idx, None)),
            Slider(info) => {
                // this is for the sliderbody
                let time = ho.start_time.as_seconds();
                hit_times.push((time, idx, None));

                let duration = beatmap
                    .get_slider_duration(ho)
                    .ok_or_else(|| anyhow!("failed to get slider duration for slider {}", ho))?;
                let single_repeat_duration = duration / info.num_repeats as f64;

                // once for each hitcircle on the slider
                for i in 0..=info.num_repeats as usize {
                    let this_time = time + (i as f64 * single_repeat_duration);
                    hit_times.push((this_time, idx, Some(i)));
                }
            }
            Spinner(info) => hit_times.push((info.end_time.as_seconds(), idx, None)),
        }
    }

    hit_times.sort_by_key(|(t, _, _)| NotNan::new(*t).unwrap());

    Ok(hit_times)
}
