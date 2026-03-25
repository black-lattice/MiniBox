use std::collections::BTreeMap;

use crate::config::external::ExternalDocument;
use crate::error::Error;

use super::model::{ClashProxy, ClashProxyGroup, ClashSubscription};

#[derive(Debug, Clone)]
struct Line {
    number: usize,
    indent: usize,
    content: String,
}

pub fn parse_document(document: &ExternalDocument) -> Result<ClashSubscription, Error> {
    let lines = collect_lines(document.raw.as_str())?;
    let mut index = 0;
    let mut parsed = ClashSubscription::default();

    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            return Err(Error::validation(format!(
                "unexpected indentation at line {} in Clash document",
                line.number
            )));
        }

        let (key, value) = split_key_value(line)?;
        match key {
            "proxies" => {
                ensure_block_value("proxies", value, line.number)?;
                index += 1;
                parsed.proxies = parse_proxies(&lines, &mut index, 2)?;
            }
            "proxy-groups" => {
                ensure_block_value("proxy-groups", value, line.number)?;
                index += 1;
                parsed.proxy_groups = parse_groups(&lines, &mut index, 2)?;
            }
            "rules" => {
                parsed.rules = parse_scalar_list_section(&lines, &mut index, value, 2)?;
            }
            "rule-providers" => {
                parsed.rule_providers = parse_mapping_section(&lines, &mut index, value, 2)?;
            }
            "proxy-providers" => {
                parsed.proxy_providers = parse_mapping_section(&lines, &mut index, value, 2)?;
            }
            "script" => {
                parsed.script = Some(parse_presence_section(&lines, &mut index, value, 2)?);
            }
            _ => skip_section(&lines, &mut index, value, 0),
        }
    }

    Ok(parsed)
}

fn collect_lines(raw: &str) -> Result<Vec<Line>, Error> {
    let mut lines = Vec::new();

    for (number, original) in raw.lines().enumerate() {
        if original.contains('\t') {
            return Err(Error::validation(format!(
                "tabs are not supported in Clash YAML input (line {})",
                number + 1
            )));
        }

        let trimmed_start = original.trim_start();
        if trimmed_start.is_empty() || trimmed_start.starts_with('#') {
            continue;
        }

        let indent = original.len() - trimmed_start.len();
        lines.push(Line {
            number: number + 1,
            indent,
            content: trimmed_start.trim_end().to_string(),
        });
    }

    Ok(lines)
}

fn parse_proxies(
    lines: &[Line],
    index: &mut usize,
    item_indent: usize,
) -> Result<Vec<ClashProxy>, Error> {
    let mut proxies = Vec::new();

    while *index < lines.len() && lines[*index].indent >= item_indent {
        let line = &lines[*index];
        if line.indent != item_indent || !line.content.starts_with('-') {
            return Err(Error::validation(format!(
                "expected proxy list item at line {}",
                line.number
            )));
        }

        let mut proxy = ClashProxy::default();
        let remainder = line.content[1..].trim_start();
        *index += 1;

        if !remainder.is_empty() {
            apply_proxy_field(&mut proxy, line, remainder, lines, index)?;
        }

        while *index < lines.len() && lines[*index].indent > item_indent {
            let nested = &lines[*index];
            if nested.indent != item_indent + 2 {
                return Err(Error::validation(format!(
                    "unexpected indentation at line {} in proxy entry",
                    nested.number
                )));
            }

            let (key, value) = split_key_value(nested)?;
            *index += 1;
            match key {
                "name" => proxy.name = required_scalar(value, nested, "proxy name")?,
                "type" => proxy.kind = required_scalar(value, nested, "proxy type")?,
                "server" => proxy.server = Some(required_scalar(value, nested, "proxy server")?),
                "port" => {
                    let value = required_scalar(value, nested, "proxy port")?;
                    proxy.port = Some(parse_port(value.as_str(), nested.number)?);
                }
                _ => {
                    if value.is_none() {
                        skip_nested_block(lines, index, nested.indent);
                    }
                }
            }
        }

        proxies.push(proxy);
    }

    Ok(proxies)
}

fn parse_groups(
    lines: &[Line],
    index: &mut usize,
    item_indent: usize,
) -> Result<Vec<ClashProxyGroup>, Error> {
    let mut groups = Vec::new();

    while *index < lines.len() && lines[*index].indent >= item_indent {
        let line = &lines[*index];
        if line.indent != item_indent || !line.content.starts_with('-') {
            return Err(Error::validation(format!(
                "expected proxy-group list item at line {}",
                line.number
            )));
        }

        let mut group = ClashProxyGroup::default();
        let remainder = line.content[1..].trim_start();
        *index += 1;

        if !remainder.is_empty() {
            apply_group_field(&mut group, line, remainder, lines, index)?;
        }

        while *index < lines.len() && lines[*index].indent > item_indent {
            let nested = &lines[*index];
            if nested.indent != item_indent + 2 {
                return Err(Error::validation(format!(
                    "unexpected indentation at line {} in proxy-group entry",
                    nested.number
                )));
            }

            let (key, value) = split_key_value(nested)?;
            *index += 1;
            match key {
                "name" => group.name = required_scalar(value, nested, "group name")?,
                "type" => group.kind = required_scalar(value, nested, "group type")?,
                "proxies" => {
                    group.proxies = parse_string_list_value(lines, index, nested, value)?;
                }
                "use" => {
                    group.r#use = parse_string_list_value(lines, index, nested, value)?;
                }
                _ => {
                    if value.is_none() {
                        skip_nested_block(lines, index, nested.indent);
                    }
                }
            }
        }

        groups.push(group);
    }

    Ok(groups)
}

fn parse_scalar_list_section(
    lines: &[Line],
    index: &mut usize,
    value: Option<&str>,
    item_indent: usize,
) -> Result<Vec<String>, Error> {
    if let Some(value) = value {
        *index += 1;
        return Ok(parse_inline_list(value).unwrap_or_else(|| vec![clean_scalar(value)]));
    }

    *index += 1;
    parse_string_list_block(lines, index, item_indent)
}

fn parse_mapping_section(
    lines: &[Line],
    index: &mut usize,
    value: Option<&str>,
    entry_indent: usize,
) -> Result<BTreeMap<String, String>, Error> {
    let mut mapping = BTreeMap::new();
    if value.is_some() {
        *index += 1;
        return Ok(mapping);
    }

    *index += 1;
    while *index < lines.len() && lines[*index].indent >= entry_indent {
        let line = &lines[*index];
        if line.indent != entry_indent {
            return Err(Error::validation(format!(
                "unexpected indentation at line {} in top-level mapping",
                line.number
            )));
        }

        let (key, value) = split_key_value(line)?;
        mapping.insert(key.to_string(), clean_scalar(value.unwrap_or_default()));
        *index += 1;
        if value.is_none() {
            skip_nested_block(lines, index, entry_indent);
        }
    }

    Ok(mapping)
}

fn parse_presence_section(
    lines: &[Line],
    index: &mut usize,
    value: Option<&str>,
    child_indent: usize,
) -> Result<String, Error> {
    let captured = value.map(clean_scalar).unwrap_or_default();
    *index += 1;
    if value.is_none() {
        skip_nested_block(lines, index, child_indent - 2);
    }
    Ok(captured)
}

fn apply_proxy_field(
    proxy: &mut ClashProxy,
    line: &Line,
    content: &str,
    _lines: &[Line],
    _index: &mut usize,
) -> Result<(), Error> {
    let (key, value) = split_inline_key_value(content, line.number)?;
    match key {
        "name" => proxy.name = clean_scalar(value),
        "type" => proxy.kind = clean_scalar(value),
        "server" => proxy.server = Some(clean_scalar(value)),
        "port" => proxy.port = Some(parse_port(value, line.number)?),
        _ => {}
    }
    Ok(())
}

fn apply_group_field(
    group: &mut ClashProxyGroup,
    line: &Line,
    content: &str,
    lines: &[Line],
    index: &mut usize,
) -> Result<(), Error> {
    let (key, value) = split_inline_key_value(content, line.number)?;
    match key {
        "name" => group.name = clean_scalar(value),
        "type" => group.kind = clean_scalar(value),
        "proxies" => {
            group.proxies = parse_inline_list(value).unwrap_or_else(|| vec![clean_scalar(value)])
        }
        "use" => {
            group.r#use = parse_inline_list(value).unwrap_or_else(|| vec![clean_scalar(value)])
        }
        _ => {
            let _ = (lines, index);
        }
    }
    Ok(())
}

fn parse_string_list_value(
    lines: &[Line],
    index: &mut usize,
    line: &Line,
    value: Option<&str>,
) -> Result<Vec<String>, Error> {
    match value {
        Some(value) => Ok(parse_inline_list(value).unwrap_or_else(|| vec![clean_scalar(value)])),
        None => parse_string_list_block(lines, index, line.indent + 2),
    }
}

fn parse_string_list_block(
    lines: &[Line],
    index: &mut usize,
    item_indent: usize,
) -> Result<Vec<String>, Error> {
    let mut items = Vec::new();

    while *index < lines.len() && lines[*index].indent >= item_indent {
        let line = &lines[*index];
        if line.indent != item_indent || !line.content.starts_with('-') {
            return Err(Error::validation(format!(
                "expected list item at line {}",
                line.number
            )));
        }

        let value = line.content[1..].trim_start();
        if value.is_empty() {
            return Err(Error::validation(format!(
                "empty list item at line {} is not supported",
                line.number
            )));
        }

        items.push(clean_scalar(value));
        *index += 1;
    }

    Ok(items)
}

fn split_key_value<'a>(line: &'a Line) -> Result<(&'a str, Option<&'a str>), Error> {
    split_inline_key_value_optional(line.content.as_str(), line.number)
}

fn split_inline_key_value<'a>(
    content: &'a str,
    line_number: usize,
) -> Result<(&'a str, &'a str), Error> {
    let (key, value) = split_inline_key_value_optional(content, line_number)?;
    let Some(value) = value else {
        return Err(Error::validation(format!(
            "expected inline value at line {}",
            line_number
        )));
    };
    Ok((key, value))
}

fn split_inline_key_value_optional<'a>(
    content: &'a str,
    line_number: usize,
) -> Result<(&'a str, Option<&'a str>), Error> {
    let Some((key, remainder)) = content.split_once(':') else {
        return Err(Error::validation(format!(
            "expected key/value pair at line {}",
            line_number
        )));
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::validation(format!(
            "empty key at line {}",
            line_number
        )));
    }

    let remainder = remainder.trim_start();
    if remainder.is_empty() {
        Ok((key, None))
    } else {
        Ok((key, Some(remainder)))
    }
}

fn required_scalar(value: Option<&str>, line: &Line, field: &str) -> Result<String, Error> {
    match value {
        Some(value) => Ok(clean_scalar(value)),
        None => Err(Error::validation(format!(
            "{field} must be a scalar value at line {}",
            line.number
        ))),
    }
}

fn parse_port(raw: &str, line_number: usize) -> Result<u16, Error> {
    raw.trim().parse::<u16>().map_err(|error| {
        Error::validation(format!("invalid port at line {}: {}", line_number, error))
    })
}

fn parse_inline_list(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return None;
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }

    Some(inner.split(',').map(clean_scalar).collect())
}

fn clean_scalar(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    stripped.to_string()
}

fn skip_section(lines: &[Line], index: &mut usize, value: Option<&str>, parent_indent: usize) {
    *index += 1;
    if value.is_none() {
        skip_nested_block(lines, index, parent_indent);
    }
}

fn skip_nested_block(lines: &[Line], index: &mut usize, parent_indent: usize) {
    while *index < lines.len() && lines[*index].indent > parent_indent {
        *index += 1;
    }
}

fn ensure_block_value(key: &str, value: Option<&str>, line_number: usize) -> Result<(), Error> {
    if value.is_some() {
        return Err(Error::validation(format!(
            "'{}' must be defined as a block at line {}",
            key, line_number
        )));
    }

    Ok(())
}
