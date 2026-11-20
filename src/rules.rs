use boss_sensor_vision::Observation;

const ENTER_SAMPLES: u8 = 2;
const EXIT_SAMPLES: u8 = 3;
const IDENTITY_SIMILARITY: f32 = 0.50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub second_face: bool,
    pub enrolled_person: bool,
}

impl Matches {
    pub fn any(self) -> bool {
        self.second_face || self.enrolled_person
    }

    pub fn label(self) -> &'static str {
        match (self.second_face, self.enrolled_person) {
            (true, true) => "检测到第二个人和指定人员",
            (true, false) => "检测到第二个人",
            (false, true) => "检测到指定人员",
            (false, false) => "等待触发",
        }
    }
}

pub fn classify<'a>(
    observation: &Observation,
    second_face: bool,
    mut identities: impl Iterator<Item = &'a [f32]>,
) -> Matches {
    Matches {
        second_face: second_face && observation.face_count >= 2,
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
    second_face: Presence,
    enrolled_person: Presence,
}

impl Rules {
    pub fn observe(&mut self, matches: Matches) -> Matches {
        Matches {
            second_face: self.second_face.observe(matches.second_face),
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
            second_face: true,
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
            classify(&observation, true, [[1.0, 0.0].as_slice()].into_iter()),
            Matches {
                second_face: false,
                enrolled_person: true
            },
        );
        assert!(!classify(&observation, true, [[1.0].as_slice()].into_iter()).any());
    }
}
