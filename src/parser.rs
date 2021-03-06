use std::fs::{File};
use std::io::{BufReader, BufRead, Read, BufWriter, Write};
use std::path::Path;

use byteorder::{self, LittleEndian, ReadBytesExt, WriteBytesExt};
use io::{Error, Result, ReadVariableExt, WriteVariableExt};
use property::{Property, PropertyMap};

static LIST_PROPERTIES: [&'static str; 5] = [
    "CombatSelects",
    "CombatSkills",
    "SkillPoints",
    "SpellFavorites",
    "SpellSkills",
];

/// Reads a property file.
pub fn read_path(path: &Path) -> Result<PropertyMap> {
    println!("Reading {:?}", path);

    let file = try!(File::open(path));
    let mut buf = BufReader::new(&file);
    let mut res = PropertyMap::new();
    loop {
        let done = try!(match buf.read_u8() {
            Ok(0x7e) => Ok(false),
            Ok(v) => Err(Error::UnexpectedTag(v)),
            Err(byteorder::Error::UnexpectedEOF) => Ok(true),
            Err(e) => Err(Error::from(e)),
        });
        if done {
            break;
        }
        let name = try!(buf.read_variable_string());
        let name_is_list = LIST_PROPERTIES.contains(&name.as_ref());
        let data_len = try!(buf.read_u32::<LittleEndian>()) as usize;
        if data_len > 0 {
            let data_type = try!(buf.read_u8());
            res.insert(name, match data_type {
                0x01 => {
                    let s = try!(buf.read_variable_string());
                    if name_is_list {
                        Property::from(s.split(",").map(String::from).collect::<Vec<String>>())
                    } else {
                        Property::from(s)
                    }
                },
                0x02 => Property::from(try!(buf.read_u32::<LittleEndian>())),
                0x06 => Property::from(try!(buf.read_f32::<LittleEndian>())),
                0x09 => Property::from(try!(buf.read_u8()) != 0),
                _    => {
                    let mut v = vec![0; data_len - 2];
                    try!(buf.read(&mut v));
                    Property::Unknown(v, data_type)
                }
            });
            buf.consume(1); // Consume end tag 0x7b
        }
    }
    Ok(res)
}

/// Writes a property file.
pub fn write_path(path: &Path, props: &PropertyMap) -> Result<()> {
    println!("Writing {:?}", path);

    let file = try!(File::create(path));
    let mut buf = BufWriter::new(&file);
    for (name, prop) in props {
        try!(buf.write_u8(0x7e));
        try!(buf.write_variable_string(name));
        match *prop {
            Property::Boolean(v) => {
                try!(buf.write_u32::<LittleEndian>(2 + 1));
                try!(buf.write_u8(0x09));
                try!(buf.write_u8(if v { 1 } else { 0 }));
            },
            Property::Integer(v) => {
                try!(buf.write_u32::<LittleEndian>(2 + 4));
                try!(buf.write_u8(0x02));
                try!(buf.write_u32::<LittleEndian>(v));
            },
            Property::Float(v) => {
                try!(buf.write_u32::<LittleEndian>(2 + 4));
                try!(buf.write_u8(0x06));
                try!(buf.write_f32::<LittleEndian>(v));
            },
            Property::String(ref v) => {
                let len = v.len() as u32;
                try!(buf.write_u32::<LittleEndian>(2 + match len.leading_zeros() {
                     1 ...  3 => 5 + len,
                     4 ... 10 => 4 + len,
                    11 ... 17 => 3 + len,
                    18 ... 24 => 2 + len,
                    25 ... 32 => 1 + len,
                    _ => panic!("string length exceeds maximum 2^31")
                }));
                try!(buf.write_u8(0x01));
                try!(buf.write_variable_string(v));
            },
            Property::List(..) => {
                let s = prop.to_string();
                let len = s.len() as u32;
                try!(buf.write_u32::<LittleEndian>(2 + match len.leading_zeros() {
                     1 ...  3 => 5 + len,
                     4 ... 10 => 4 + len,
                    11 ... 17 => 3 + len,
                    18 ... 24 => 2 + len,
                    25 ... 32 => 1 + len,
                    _ => panic!("string length exceeds maximum 2^31")
                }));
                try!(buf.write_u8(0x01));
                try!(buf.write_variable_string(&s));
            },
            Property::Unknown(ref v, data_type) => {
                let len = v.len() as u32;
                try!(buf.write_u32::<LittleEndian>(2 + len));
                try!(buf.write_u8(data_type));
                try!(buf.write_all(v));
            },
        };
        try!(buf.write_u8(0x7b));
    }
    Ok(())
}

