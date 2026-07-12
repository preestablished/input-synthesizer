//! Deep-merge of sparse overrides onto a base config (API.md §5 tail):
//! maps merge recursively, scalars and lists replace.

use super::{ConfigError, ExperimentConfig};

pub fn deep_merge(
    base: &ExperimentConfig,
    overrides_yaml: &[u8],
) -> Result<ExperimentConfig, ConfigError> {
    if overrides_yaml.iter().all(u8::is_ascii_whitespace) {
        return Ok(base.clone());
    }
    let mut base_value =
        serde_yaml::to_value(base).map_err(|e| ConfigError::Invalid(e.to_string()))?;
    let overrides: serde_yaml::Value = serde_yaml::from_slice(overrides_yaml).map_err(|e| {
        let loc = e.location();
        ConfigError::Parse {
            message: format!("config_overrides_yaml: {e}"),
            line: loc.as_ref().map(|l| l.line()),
            column: loc.as_ref().map(|l| l.column()),
        }
    })?;
    merge_value(&mut base_value, overrides);
    serde_yaml::from_value(base_value).map_err(|e| ConfigError::Parse {
        message: format!("config_overrides_yaml produced an invalid document: {e}"),
        line: None,
        column: None,
    })
}

fn merge_value(base: &mut serde_yaml::Value, overrides: serde_yaml::Value) {
    match (base, overrides) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(over_map)) => {
            for (key, over_val) in over_map {
                match base_map.get_mut(&key) {
                    Some(base_val) => merge_value(base_val, over_val),
                    None => {
                        base_map.insert(key, over_val);
                    }
                }
            }
        }
        (base_slot, over_val) => *base_slot = over_val,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{parse, ModelKindCfg};
    use super::*;

    const MINIMAL: &str = r#"
version: 1
kind: experiment_config
experiment_id: exp-test
model: pad
button_alphabet:
  name: console16-12btn-v1
  buttons:
    [ {name: A, bit: 0}, {name: B, bit: 1} ]
"#;

    #[test]
    fn maps_merge_scalars_replace() {
        let base = parse(MINIMAL.as_bytes()).unwrap();
        let merged = deep_merge(
            &base,
            b"generator_mix: { weighted_random: 1.0, macro: 0.0 }\nburst_len: { mean_frames: 100 }",
        )
        .unwrap();
        assert_eq!(merged.generator_mix.weighted_random, 1.0);
        assert_eq!(merged.generator_mix.macro_, 0.0);
        // untouched sibling key in a merged map keeps base value
        assert_eq!(merged.generator_mix.mutation, 0.20);
        assert_eq!(merged.burst_len.mean_frames, 100);
        assert_eq!(merged.burst_len.max_frames, 1800);
        assert_eq!(merged.model, ModelKindCfg::Pad);
    }

    #[test]
    fn lists_replace_entirely() {
        let base = parse(MINIMAL.as_bytes()).unwrap();
        let merged = deep_merge(
            &base,
            b"button_alphabet: { buttons: [ {name: X, bit: 2} ] }",
        )
        .unwrap();
        assert_eq!(merged.button_alphabet.buttons.len(), 1);
        assert_eq!(merged.button_alphabet.buttons[0].name, "X");
        // sibling scalar under the same map is preserved
        assert_eq!(merged.button_alphabet.name, "console16-12btn-v1");
    }

    #[test]
    fn empty_overrides_are_a_noop() {
        let base = parse(MINIMAL.as_bytes()).unwrap();
        assert_eq!(deep_merge(&base, b"  \n").unwrap(), base);
    }

    #[test]
    fn malformed_overrides_error_names_the_source() {
        let base = parse(MINIMAL.as_bytes()).unwrap();
        let err = deep_merge(&base, b"{ not yaml").unwrap_err();
        assert!(err.to_string().contains("config_overrides_yaml"));
    }
}
