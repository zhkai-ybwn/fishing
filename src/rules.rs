use boss_sensor_vision::Observation;

const ENTER_SAMPLES: u8 = 2;
const EXIT_SAMPLES: u8 = 3;
const IDENTITY_SIMILARITY: f32 = 0.50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub face_count: bool,
    pub enrolled_person: bool,
}

impl Matches {
    pub fn any(self) -> bool {
        self.face_count || self.enrolled_person
    }

    pub fn label(self) -> &'static str {
        match (self.face_count, self.enrolled_person) {
            (true, true) => "人数和指定人员条件均已触发",
            (true, false) => "人脸数量达到阈值",
            (false, true) => "检测到指定人员",
            (false, false) => "等待触发",
        }
    }
}

pub fn classify<'a>(
    observation: &Observation,
    minimum_faces: Option<u32>,
    mut identities: impl Iterator<Item = &'a [f32]>,
) -> Matches {
    Matches {
        face_count: minimum_faces.is_some_and(|minimum| observation.face_count >= minimum as usize),
        enrolled_person: identities.any(|identity| {
            observation.embeddings.iter().any(|candidate| {
                candidate.len() == identity.len()
                    && candidate
                        .iter()
                        .zip(identity)
                        .map(|(a, b)| a * b)
                        .sum::<f32>()
                        >= IDENTITY_SIMILARITY
            })
        }),
    }
}

#[derive(Default)]
struct Presence {
    active: bool,
    differing_samples: u8,
}

impl Presence {
    fn observe(&mut self, matched: bool) -> bool {
        if matched == self.active {
            self.differing_samples = 0;
            return false;
        }

        self.differing_samples += 1;
        let required_samples = if matched { ENTER_SAMPLES } else { EXIT_SAMPLES };
        if self.differing_samples < required_samples {
            return false;
        }

        self.differing_samples = 0;
        self.active = matched;
        matched
    }
}

#[derive(Default)]
pub struct Rules {
    face_count: Presence,
    enrolled_person: Presence,
}

impl Rules {
    pub fn observe(&mut self, matches: Matches) -> Matches {
        Matches {
            face_count: self.face_count.observe(matches.face_count),
            enrolled_person: self.enrolled_person.observe(matches.enrolled_person),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_matches_emit_one_event_and_rearm_after_departure() {
        let mut rules = Rules::default();
        let both = Matches {
            face_count: true,
            enrolled_person: true,
        };
        let empty = Matches::default();
        assert_eq!(rules.observe(both), empty);
        assert_eq!(rules.observe(both), both);
        assert_eq!(rules.observe(both), empty);
        assert_eq!(rules.observe(empty), empty);
        assert_eq!(rules.observe(both), empty);
        for _ in 0..EXIT_SAMPLES {
            assert_eq!(rules.observe(empty), empty);
        }
        assert_eq!(rules.observe(both), empty);
        assert_eq!(rules.observe(both), both);
    }

    #[test]
    fn one_enrolled_face_matches_independently_of_face_count() {
        let observation = Observation {
            face_count: 1,
            embeddings: vec![vec![0.6, 0.8]],
        };
        assert_eq!(
            classify(&observation, Some(2), [[1.0, 0.0].as_slice()].into_iter()),
            Matches {
                face_count: false,
                enrolled_person: true
            },
        );
        assert!(!classify(&observation, Some(2), [[1.0].as_slice()].into_iter()).any());
    }

    #[test]
    fn count_rule_accepts_custom_minimums_and_can_be_disabled() {
        let observation = Observation {
            face_count: 3,
            embeddings: Vec::new(),
        };
        for minimum in [Some(1), Some(3)] {
            assert!(classify(&observation, minimum, std::iter::empty()).face_count);
        }
        for minimum in [Some(4), Some(u32::MAX), None] {
            assert!(!classify(&observation, minimum, std::iter::empty()).face_count);
        }
    }
}
