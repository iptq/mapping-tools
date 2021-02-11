use std::fmt::Debug;
use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use libosu::prelude::*;
use same_file::is_same_file;

#[derive(Debug, StructOpt)]
pub struct CopyHitsoundOpts {
    /// The path of the map to copy hitsounds from.
    pub src: PathBuf,

    /// The paths of maps to copy hitsounds to.
    pub dsts: Vec<PathBuf>,

    #[structopt(flatten)]
    pub extra: ExtraOpts,
}

#[derive(Default, Debug, StructOpt)]
pub struct ExtraOpts {
    /// Temporal leniency, the number of milliseconds apart two objects can be apart
    #[structopt(short = "l", long = "leniency", default_value = "2")]
    pub leniency: u32,
}

pub fn copy_hitsounds_cmd(opts: CopyHitsoundOpts) -> Result<()> {
    let file = File::open(&opts.src)?;
    let src_beatmap = Beatmap::parse(file)?;

    let mut dst_beatmaps = Vec::new();
    for dst in opts.dsts.iter() {
        // don't overwrite the source file
        if is_same_file(&opts.src, dst)? {
            continue;
        }

        let file = File::open(dst)?;
        dst_beatmaps.push(Beatmap::parse(file)?);
    }

    copy_hitsounds(&src_beatmap, &mut dst_beatmaps, opts.extra)?;

    for (path, beatmap) in opts.dsts.iter().zip(dst_beatmaps) {
        // don't overwrite the source file
        if is_same_file(&opts.src, path)? {
            continue;
        }

        {
            let file = File::create(path)?;
            beatmap.write(file)?;
        }
    }

    Ok(())
}

pub fn copy_hitsounds(src: &Beatmap, dsts: &mut Vec<Beatmap>, opts: ExtraOpts) -> Result<()> {
    let hitsound_data = collect_hitsounds(&src, &opts)?;

    for dst in dsts.iter_mut() {
        apply_hitsounds(&hitsound_data, dst, &opts)?;
    }

    Ok(())
}

/// All information about the hitsounds in a single file, that can be used to copy to another
/// beatmap without more information from the original beatmap.
#[derive(Debug)]
pub struct HitsoundData {
    pub hits: Vec<HitsoundInfo>,
    pub tps: Vec<SectionProps>,
}

/// Hitsound data for a single instant
#[derive(Debug)]
pub struct HitsoundInfo {
    pub time: f64,
    pub additions: Additions,
    pub sample_info: SampleInfo,
}

/// Properties of a timing section any time it can be changed
#[derive(Debug)]
pub struct SectionProps {
    pub time: f64,
    pub vol: u16,
    pub kiai: bool,
    pub sample_index: u32,
}

/// Returns all the information extracted from the beatmap that can be used to copy hitsounds to a
/// different beatmap.
fn collect_hitsounds(beatmap: &Beatmap, _opts: &ExtraOpts) -> Result<HitsoundData> {
    let mut hits = Vec::new();
    let hit_times = get_hit_times(beatmap, false)?;

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
                info.edge_additions
                    .get(repeat_idx)
                    .cloned()
                    .unwrap_or(ho.additions)
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
                if let Some(samplesets) = info.edge_samplesets.get(repeat_idx) {
                    sample_info.sample_set = samplesets.0;
                    sample_info.addition_set = samplesets.1;
                }
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

    let mut tps = Vec::new();
    for tp in beatmap.timing_points.iter() {
        tps.push(SectionProps {
            time: tp.time.as_seconds(),
            vol: tp.volume,
            kiai: tp.kiai,
            sample_index: tp.sample_index,
        });
    }
    tps.sort_by_key(|tp| NotNan::new(tp.time).unwrap());

    Ok(HitsoundData { hits, tps })
}

/// Given a set of hitsound data, and a beatmap, applies the hitsound data to the beatmap.
fn apply_hitsounds(
    hitsound_data: &HitsoundData,
    beatmap: &mut Beatmap,
    opts: &ExtraOpts,
) -> Result<()> {
    // doesn't hurt to make sure that these lists are sorted
    beatmap.hit_objects.sort_by_key(|ho| ho.start_time);
    beatmap.timing_points.sort_by_key(|tp| tp.time);

    let leniency = Millis(opts.leniency as i32).as_seconds();

    let hit_times = get_hit_times(&beatmap, false)?;
    for (hit_time, ho_idx, repeat_idx) in hit_times {
        // determine the hit using binary search over the collected data
        let hit = match binary_search_for(hit_time, &hitsound_data.hits, |hit| hit.time, leniency) {
            Ok(idx) => &hitsound_data.hits[idx],
            Err(_) => {
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
    for tp in hitsound_data.tps.iter() {
        let map_tp = match binary_search_for(
            tp.time,
            &beatmap.timing_points,
            |tp| tp.time.as_seconds(),
            leniency,
        ) {
            Ok(idx) => &mut beatmap.timing_points[idx],
            Err(idx) => {
                let tp = beatmap.timing_points[idx].clone();
                beatmap.timing_points.insert(idx, tp);
                &mut beatmap.timing_points[idx]
            }
        };

        map_tp.sample_index = tp.sample_index;
        map_tp.volume = tp.vol;
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

/// Performs binary search using a leniency measure instead of exact equality.
fn binary_search_for<T, F>(
    needle: f64,
    haystack: &[T],
    extract: F,
    leniency: f64,
) -> Result<usize, usize>
where
    T: Debug,
    F: Fn(&T) -> f64,
{
    use std::cmp::Ordering::{self, *};

    trace!("binary searching for {}", needle);
    let mut size = haystack.len();
    if size == 0 {
        return Err(0);
    }

    // special cmp function that takes leniency into account
    let cmp = move |a: f64, b: f64| -> Ordering {
        if (a - b).abs() < leniency {
            Equal
        } else if a < b {
            Less
        } else {
            Greater
        }
    };

    let mut base = 0usize;
    while size > 1 {
        let half = size / 2;
        let mid = base + half;
        let el = unsafe { haystack.get_unchecked(mid) };
        let t = extract(el);

        let ord = cmp(t, needle);
        base = if ord == Greater { base } else { mid };
        size -= half;
    }

    let el = unsafe { haystack.get_unchecked(base) };
    let t = extract(el);

    let ord = cmp(t, needle);
    if ord == Equal {
        Ok(base)
    } else {
        Err(base + (ord == Less) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::binary_search_for;

    #[test]
    fn test_binary_search() {
        let id = |f: &f64| *f;
        let list = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(binary_search_for(2.05, &list, id, 0.1), Ok(2));
        assert_eq!(binary_search_for(1.95, &list, id, 0.1), Ok(2));
        assert_eq!(binary_search_for(2.05, &list, id, 0.03), Err(3));
        assert_eq!(binary_search_for(1.95, &list, id, 0.03), Err(2));
    }
}

/// Collect a list of EVERY possible time a hitsound could be played
///
/// This includes hitcircles, and every repeat / tail of sliders. The return value is
/// (timestamp in seconds, index of hitobject, index of repeat (if slider))
///
/// Notably, this assumes that the hit_objects is sorted, since it refers to hit_objects by index
fn get_hit_times(beatmap: &Beatmap, slider_body: bool) -> Result<Vec<(f64, usize, Option<usize>)>> {
    let mut hit_times = Vec::new();

    for (idx, ho) in beatmap.hit_objects.iter().enumerate() {
        use HitObjectKind::*;
        match &ho.kind {
            Circle => hit_times.push((ho.start_time.as_seconds(), idx, None)),
            Slider(info) => {
                let time = ho.start_time.as_seconds();
                if slider_body {
                    // this is for the sliderbody
                    hit_times.push((time, idx, None));
                }

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
