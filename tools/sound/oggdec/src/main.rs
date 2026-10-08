use lewton::inside_ogg::OggStreamReader;
use std::fs::File;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (inp, out) = (&args[1], &args[2]);
    let mut r = OggStreamReader::new(File::open(inp).unwrap()).unwrap();
    let ch = r.ident_hdr.audio_channels as u16;
    let sr = r.ident_hdr.audio_sample_rate;
    let mut pcm: Vec<i16> = Vec::new();
    while let Some(p) = r.read_dec_packet_itl().unwrap() {
        pcm.extend_from_slice(&p);
    }
    let bytes = (pcm.len() * 2) as u32;
    let mut f = File::create(out).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&ch.to_le_bytes()).unwrap();
    f.write_all(&sr.to_le_bytes()).unwrap();
    f.write_all(&(sr * ch as u32 * 2).to_le_bytes()).unwrap();
    f.write_all(&(ch * 2).to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&bytes.to_le_bytes()).unwrap();
    for s in pcm {
        f.write_all(&s.to_le_bytes()).unwrap();
    }
    println!("{inp}: {sr} Hz, {ch} ch");
}
