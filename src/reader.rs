use spec;
use crc32::Crc32Reader;
use std::io;
use std::io::{IoResult, IoError};
use flate2::FlateReader;

pub struct ZipContainer<T>
{
    inner: T,
    files: Vec<ZipFile>,
}

struct ZipFile
{
    central_header: spec::CentralDirectoryHeader,
    local_header: spec::LocalFileHeader,
    _data_descriptor: Option<spec::DataDescriptor>,
}

pub struct ZipFileItems<'a, T:'a>
{
    container: &'a mut ZipContainer<T>,
    pos: uint,
}

pub struct ZipFileItem<'a>
{
    pub name: String,
    pub size: uint,
    pub reader: Box<Reader+'a>,
}

fn unsupported_zip_error<T>(detail: String) -> IoResult<T>
{
    Err(IoError
        {
            kind: io::OtherIoError,
            desc: "This ZIP file is not supported",
            detail: Some(detail),
        })
}

impl<T: Reader+Seek> ZipContainer<T>
{
    pub fn new(inner: T) -> IoResult<ZipContainer<T>>
    {
        let mut result = ZipContainer { inner: inner, files: Vec::new() };
        let footer = try!(spec::CentralDirectoryEnd::find_and_parse(&mut result.inner));

        if footer.number_of_disks > 1 { return unsupported_zip_error("Support for multi-disk files is not implemented".to_string()) }

        let directory_start = footer.central_directory_offset as i64;
        let number_of_files = footer.number_of_files_on_this_disk as uint;

        let mut files = Vec::with_capacity(number_of_files);

        try!(result.inner.seek(directory_start, io::SeekSet));
        for i in range(0, number_of_files)
        {
            files.push(try!(ZipContainer::parse_directory(&mut result.inner)));
        }

        result.files = files;
        Ok(result)
    }

    fn parse_directory(reader: &mut T) -> IoResult<ZipFile>
    {
        let cdh = try!(spec::CentralDirectoryHeader::parse(reader));
        let pos = try!(reader.tell()) as i64;
        let result = ZipFile::new(reader, cdh);
        try!(reader.seek(pos, io::SeekSet));
        result
    }

    pub fn files<'a>(&'a mut self) -> ZipFileItems<'a, T>
    {
        ZipFileItems { container: self, pos: 0 }
    }
}

impl ZipFile
{
    pub fn new<T: Reader+Seek>(reader: &mut T, central_directory_header: spec::CentralDirectoryHeader) -> IoResult<ZipFile>
    {
        try!(reader.seek(central_directory_header.file_offset as i64, io::SeekSet));
        let lfh = try!(spec::LocalFileHeader::parse(reader));
        let desc = if lfh.has_descriptor
        {
            try!(reader.seek(lfh.compressed_size as i64, io::SeekCur));
            Some(try!(spec::DataDescriptor::parse(reader)))
        }
        else { None };


        Ok(ZipFile { central_header: central_directory_header, local_header: lfh, _data_descriptor: desc })
    }
}

impl<'a> ZipFileItem<'a>
{
    fn new<S: Reader+Seek>(reader: &mut S, file: &ZipFile) -> IoResult<ZipFileItem<'a>>
    {
        let fname = file.central_header.file_name.clone();
        let name = String::from_utf8(fname).unwrap_or("???".to_string());
        let pos = file.local_header.header_end as i64;

        try!(reader.seek(pos, io::SeekSet));
        let lreader = io::util::LimitReader::new(reader.by_ref(), file.central_header.compressed_size as uint);

        let reader = match file.central_header.compression_method
        {
            spec::Stored => box Crc32Reader::new_with_check(lreader, file.central_header.crc32) as Box<Reader>,
            spec::Deflated => box Crc32Reader::new_with_check(lreader.deflate_decode(), file.central_header.crc32) as Box<Reader>,
            _ => return unsupported_zip_error("Compression method not supported".to_string()),
        };

        Ok(ZipFileItem { name: name, reader: reader, size: file.central_header.uncompressed_size as uint })
    }
}

impl<'a, T: Reader+Seek> Iterator<ZipFileItem<'a>> for ZipFileItems<'a, T>
{
    fn next(&mut self) -> Option<ZipFileItem<'a>>
    {
        self.pos += 1;
        if self.pos - 1 >= self.container.files.len()
        {
            None
        }
        else
        {
            let result = ZipFileItem::new(&mut self.container.inner, &self.container.files[self.pos - 1]);
            result.ok()
        }
    }
}