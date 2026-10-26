use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use quick_xml::escape::{escape, resolve_predefined_entity};
use quick_xml::events::{BytesStart, Event};
use quick_xml::XmlVersion;
use serde_json::{Map, Number, Value};

use crate::convert::{io_failure, Failure};

// Cada cuántas filas se informa del avance y se comprueba si han cancelado
const TICK_ROWS: u64 = 20_000;

// CSV y TSV se procesan fila a fila; JSON, YAML y XML necesitan el árbol completo en memoria
pub fn convert(
    input: &Path,
    from: &str,
    output: &Path,
    to: &str,
    mut tick: impl FnMut(f64) -> bool,
) -> Result<(), Failure> {
    let file = File::create_new(output).map_err(|e| io_failure(&e))?;
    let mut out = BufWriter::new(file);
    if matches!(from, "csv" | "tsv") && matches!(to, "csv" | "tsv" | "json") {
        stream_table(input, from, &mut out, to, &mut tick)?;
    } else {
        let value = read_value(input, from)?;
        write_value(&value, &mut out, to)?;
    }
    out.flush().map_err(|e| io_failure(&e))
}

fn delimiter(ext: &str) -> u8 {
    if ext == "tsv" {
        b'\t'
    } else {
        b','
    }
}

fn table_reader(input: &Path, ext: &str) -> Result<csv::Reader<File>, Failure> {
    let file = File::open(input).map_err(|e| io_failure(&e))?;
    Ok(csv::ReaderBuilder::new()
        .delimiter(delimiter(ext))
        .flexible(true)
        .from_reader(file))
}

// Las hojas exportadas desde Excel no siempre son UTF-8: mejor un carácter raro que fallar
fn headers(reader: &mut csv::Reader<File>) -> Result<Vec<String>, Failure> {
    let row = reader.byte_headers().map_err(unreadable)?;
    Ok(row
        .iter()
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect())
}

fn stream_table(
    input: &Path,
    from: &str,
    out: &mut impl Write,
    to: &str,
    tick: &mut impl FnMut(f64) -> bool,
) -> Result<(), Failure> {
    let size = input.metadata().map(|m| m.len()).unwrap_or(0).max(1) as f64;
    let mut reader = table_reader(input, from)?;
    let headers = headers(&mut reader)?;
    let mut record = csv::ByteRecord::new();
    let mut rows = 0u64;
    let mut keep_going = |reader: &csv::Reader<File>, rows: u64| {
        rows % TICK_ROWS != 0 || tick(reader.position().byte() as f64 / size)
    };

    if to == "json" {
        out.write_all(b"[").map_err(|e| io_failure(&e))?;
        while reader.read_byte_record(&mut record).map_err(unreadable)? {
            let separator: &[u8] = if rows == 0 { b"\n  " } else { b",\n  " };
            out.write_all(separator).map_err(|e| io_failure(&e))?;
            serde_json::to_writer(&mut *out, &row_object(&headers, &record))
                .map_err(|e| io_failure(&e.into()))?;
            rows += 1;
            if !keep_going(&reader, rows) {
                return Err(Failure::Cancelled);
            }
        }
        out.write_all(b"\n]\n").map_err(|e| io_failure(&e))
    } else {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter(to))
            .from_writer(out);
        writer.write_record(&headers).map_err(csv_failure)?;
        while reader.read_byte_record(&mut record).map_err(unreadable)? {
            writer.write_byte_record(&record).map_err(csv_failure)?;
            rows += 1;
            if !keep_going(&reader, rows) {
                return Err(Failure::Cancelled);
            }
        }
        writer.flush().map_err(|e| io_failure(&e))
    }
}

fn row_object(headers: &[String], record: &csv::ByteRecord) -> Value {
    let mut row = Map::new();
    for (i, field) in record.iter().enumerate() {
        let key = headers
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("column{}", i + 1));
        row.insert(key, guess(&String::from_utf8_lossy(field)));
    }
    Value::Object(row)
}

// Números y booleanos vuelven a serlo, pero solo si no cambian al escribirlos ("007" sigue siendo texto)
fn guess(text: &str) -> Value {
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = text.parse::<i64>() {
        if n.to_string() == text {
            return Value::from(n);
        }
    }
    if let Some(n) = text.parse::<f64>().ok().and_then(Number::from_f64) {
        if n.to_string() == text {
            return Value::Number(n);
        }
    }
    Value::String(text.to_string())
}

fn read_value(input: &Path, from: &str) -> Result<Value, Failure> {
    let file = File::open(input).map_err(|e| io_failure(&e))?;
    let reader = BufReader::new(file);
    match from {
        "json" => serde_json::from_reader(reader).map_err(unreadable),
        "yaml" => serde_yaml::from_reader(reader).map_err(unreadable),
        "xml" => read_xml(reader),
        _ => {
            let mut table = table_reader(input, from)?;
            let headers = headers(&mut table)?;
            let mut rows = Vec::new();
            for record in table.byte_records() {
                rows.push(row_object(&headers, &record.map_err(unreadable)?));
            }
            Ok(Value::Array(rows))
        }
    }
}

fn write_value(value: &Value, out: &mut impl Write, to: &str) -> Result<(), Failure> {
    match to {
        "json" => serde_json::to_writer_pretty(out, value).map_err(|e| io_failure(&e.into())),
        "yaml" => serde_yaml::to_writer(out, value).map_err(|_| Failure::Failed),
        "xml" => write_xml(value, out).map_err(|e| io_failure(&e)),
        _ => write_table(value, out, to),
    }
}

// Un elemento XML: atributos como "@nombre", hijos repetidos como lista y texto mixto como "#text"
struct Element {
    name: String,
    fields: Map<String, Value>,
    text: String,
}

impl Element {
    fn open(tag: &BytesStart) -> Result<Element, Failure> {
        let mut fields = Map::new();
        for attr in tag.attributes() {
            let attr = attr.map_err(unreadable)?;
            let key = format!("@{}", attr.key.as_ref());
            let value = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(unreadable)?;
            fields.insert(key, Value::String(value.into_owned()));
        }
        Ok(Element {
            name: tag.name().as_ref().to_string(),
            fields,
            text: String::new(),
        })
    }

    fn finish(mut self) -> (String, Value) {
        if self.fields.is_empty() {
            return (self.name, Value::String(self.text));
        }
        if !self.text.is_empty() {
            self.fields.insert("#text".into(), Value::String(self.text));
        }
        (self.name, Value::Object(self.fields))
    }
}

fn read_xml(input: impl BufRead) -> Result<Value, Failure> {
    let mut reader = quick_xml::Reader::from_reader(input);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut open: Vec<Element> = Vec::new();
    let mut root = None;

    loop {
        match reader.read_event_into(&mut buf).map_err(unreadable)? {
            Event::Start(tag) => open.push(Element::open(&tag)?),
            Event::Empty(tag) => attach(&mut open, &mut root, Element::open(&tag)?),
            Event::End(_) => {
                let element = open.pop().ok_or(Failure::Unreadable)?;
                attach(&mut open, &mut root, element);
            }
            Event::Text(text) => push_text(&mut open, &text.xml10_content()),
            Event::CData(data) => push_text(&mut open, &data.xml10_content()),
            Event::GeneralRef(entity) => {
                let resolved = match entity.resolve_char_ref().map_err(unreadable)? {
                    Some(ch) => ch.to_string(),
                    None => resolve_predefined_entity(&entity)
                        .ok_or(Failure::Unreadable)?
                        .to_string(),
                };
                push_text(&mut open, &resolved);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    root.ok_or(Failure::Unreadable)
}

fn push_text(open: &mut [Element], text: &str) {
    if let Some(element) = open.last_mut() {
        element.text.push_str(text);
    }
}

fn attach(open: &mut [Element], root: &mut Option<Value>, element: Element) {
    let (name, value) = element.finish();
    let Some(parent) = open.last_mut() else {
        *root = Some(Value::Object(Map::from_iter([(name, value)])));
        return;
    };
    match parent.fields.get_mut(&name) {
        Some(Value::Array(items)) => items.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
        None => {
            parent.fields.insert(name, value);
        }
    }
}

fn write_xml(value: &Value, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    match value {
        Value::Object(map) if map.len() == 1 && !map.values().all(Value::is_array) => {
            let (name, inner) = map.iter().next().unwrap();
            write_element(out, name, inner, 0)
        }
        // Un XML necesita una sola raíz
        Value::Array(_) => {
            writeln!(out, "<root>")?;
            write_element(out, "item", value, 1)?;
            writeln!(out, "</root>")
        }
        _ => write_element(out, "root", value, 0),
    }
}

fn write_element(out: &mut impl Write, name: &str, value: &Value, depth: usize) -> io::Result<()> {
    let name = xml_name(name);
    let pad = "  ".repeat(depth);
    match value {
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| write_element(out, &name, item, depth)),
        Value::Object(map) => {
            write!(out, "{pad}<{name}")?;
            for (key, attr) in map {
                if let Some(attr_name) = key.strip_prefix('@') {
                    let text = plain_text(attr);
                    write!(
                        out,
                        " {}=\"{}\"",
                        xml_name(attr_name),
                        escape(text.as_str())
                    )?;
                }
            }
            let children: Vec<_> = map.iter().filter(|(k, _)| !k.starts_with('@')).collect();
            match children.as_slice() {
                [] => writeln!(out, "/>"),
                [(key, text)] if *key == "#text" => {
                    writeln!(out, ">{}</{name}>", escape(plain_text(text).as_str()))
                }
                _ => {
                    writeln!(out, ">")?;
                    for (key, child) in children {
                        if key == "#text" {
                            writeln!(out, "{pad}  {}", escape(plain_text(child).as_str()))?;
                        } else {
                            write_element(out, key, child, depth + 1)?;
                        }
                    }
                    writeln!(out, "{pad}</{name}>")
                }
            }
        }
        Value::Null => writeln!(out, "{pad}<{name}/>"),
        scalar => writeln!(
            out,
            "{pad}<{name}>{}</{name}>",
            escape(plain_text(scalar).as_str())
        ),
    }
}

// Las cabeceras de un CSV suelen tener espacios, que no valen como nombre de etiqueta
fn xml_name(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !clean.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        clean.insert(0, '_');
    }
    clean
}

fn plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// Busca la lista de registros: en un XML o YAML suele estar envuelta en una o dos claves
fn records(mut value: &Value) -> Vec<&Value> {
    loop {
        match value {
            Value::Array(items) => return items.iter().collect(),
            Value::Object(map) if map.len() == 1 => value = map.values().next().unwrap(),
            other => return vec![other],
        }
    }
}

fn write_table(value: &Value, out: &mut impl Write, to: &str) -> Result<(), Failure> {
    let rows = records(value);
    let mut columns: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for row in &rows {
        let keys: Vec<String> = match row {
            Value::Object(map) => map.keys().cloned().collect(),
            _ => vec!["value".into()],
        };
        for key in keys {
            if seen.insert(key.clone()) {
                columns.push(key);
            }
        }
    }

    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter(to))
        .from_writer(out);
    writer.write_record(&columns).map_err(csv_failure)?;
    for row in rows {
        let cells = columns.iter().map(|column| match row {
            Value::Object(map) => map.get(column).map(plain_text).unwrap_or_default(),
            other => plain_text(other),
        });
        writer.write_record(cells).map_err(csv_failure)?;
    }
    writer.flush().map_err(|e| io_failure(&e))
}

fn unreadable(error: impl std::fmt::Display) -> Failure {
    eprintln!("datos ilegibles: {error}");
    Failure::Unreadable
}

fn csv_failure(error: csv::Error) -> Failure {
    match error.into_kind() {
        csv::ErrorKind::Io(e) => io_failure(&e),
        _ => Failure::Failed,
    }
}
