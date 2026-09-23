use anyhow::{Result, ensure};
use whisker_plugin::project::PropertyListValue;
pub(crate) fn plist_xml(value: &PropertyListValue) -> Result<String> {
    fn convert(v: &PropertyListValue) -> Result<plist::Value> {
        Ok(match v {
            PropertyListValue::String(v) => {
                ensure!(
                    v.chars().all(|c| c >= ' ' || "\n\r\t".contains(c)),
                    "invalid XML control character"
                );
                plist::Value::String(v.clone())
            }
            PropertyListValue::Integer(v) => plist::Value::Integer((*v).into()),
            PropertyListValue::Real(v) => {
                ensure!(v.is_finite(), "non-finite plist real");
                plist::Value::Real(*v)
            }
            PropertyListValue::Boolean(v) => plist::Value::Boolean(*v),
            PropertyListValue::Data(v) => plist::Value::Data(v.clone()),
            PropertyListValue::Date(v) => plist::Value::Date(plist::Date::from_xml_format(v)?),
            PropertyListValue::Array(v) => {
                plist::Value::Array(v.iter().map(convert).collect::<Result<_>>()?)
            }
            PropertyListValue::Dict(v) => plist::Value::Dictionary(
                v.iter()
                    .map(|(k, v)| {
                        ensure!(
                            k.chars().all(|c| c >= ' ' || "\n\r\t".contains(c)),
                            "invalid XML dictionary key"
                        );
                        Ok((k.clone(), convert(v)?))
                    })
                    .collect::<Result<_>>()?,
            ),
        })
    }
    let mut bytes = Vec::new();
    convert(value)?.to_writer_xml(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}
