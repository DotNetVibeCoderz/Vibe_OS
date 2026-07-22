//! Minimal static ELF64 loader for ring-3 user programs (v0.4 "Tunas").
//! Maps PT_LOAD segments as user-accessible pages into the current address
//! space and returns the entry point. Only what the bflat/zerolib output
//! needs: ET_EXEC, x86-64, PT_LOAD segments, no relocations, no interpreter.

use alloc::vec::Vec;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

fn read_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn read_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn read_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7],
    ])
}

pub struct LoadedProgram {
    pub entry: u64,
    /// Virtual addresses of every page mapped for this program, so it can be
    /// unmapped on exit (see [`crate::memory::unmap_user_pages`]).
    pub pages: Vec<u64>,
}

/// Load `image` into the address space governed by `mapper`, allocating frames
/// from `frame_allocator`. Returns the entry point virtual address.
pub fn load(
    image: &[u8],
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<LoadedProgram, &'static str> {
    if image.len() < 64 || &image[0..4] != b"\x7FELF" {
        return Err("not an ELF file");
    }
    if image[4] != 2 {
        return Err("not ELF64");
    }
    if read_u16(image, 16) != 2 {
        return Err("not ET_EXEC");
    }
    if read_u16(image, 18) != 0x3E {
        return Err("not x86-64");
    }

    let entry = read_u64(image, 24);
    let phoff = read_u64(image, 32) as usize;
    let phentsize = read_u16(image, 54) as usize;
    let phnum = read_u16(image, 56) as usize;

    let mut pages = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if read_u32(image, ph) != PT_LOAD {
            continue;
        }
        let flags = read_u32(image, ph + 4);
        let offset = read_u64(image, ph + 8) as usize;
        let vaddr = read_u64(image, ph + 16);
        let filesz = read_u64(image, ph + 32) as usize;
        let memsz = read_u64(image, ph + 40) as usize;
        map_segment(
            image, offset, vaddr, filesz, memsz, flags, mapper, frame_allocator, &mut pages,
        )?;
    }

    Ok(LoadedProgram { entry, pages })
}

/// Map one PT_LOAD segment: allocate frames page by page, copy file bytes,
/// zero the rest (bss), and map user-accessible. Parent table entries are
/// marked user-accessible too, or ring 3 would fault walking them.
#[allow(clippy::too_many_arguments)]
fn map_segment(
    image: &[u8],
    offset: usize,
    vaddr: u64,
    filesz: usize,
    memsz: usize,
    flags: u32,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    pages: &mut Vec<u64>,
) -> Result<(), &'static str> {
    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if flags & PF_W != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if flags & PF_X == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    let parent_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    let start = vaddr & !0xFFF;
    let end = vaddr + memsz as u64;
    let mut page_addr = start;
    while page_addr < end {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(page_addr));
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("out of frames loading ELF")?;

        // Zero the frame, then copy the slice of the segment that lands in it.
        // Access it through the kernel's identity/offset mapping via a
        // temporary writable kernel mapping is avoided by writing before the
        // user mapping is installed: we use the physical-memory offset map.
        let phys = frame.start_address().as_u64();
        let dst = crate::memory::phys_to_virt(phys) as *mut u8;
        unsafe { core::ptr::write_bytes(dst, 0, 4096) };

        // Bytes of this page that fall within [vaddr, vaddr+filesz).
        let page_end = page_addr + 4096;
        let copy_lo = core::cmp::max(page_addr, vaddr);
        let copy_hi = core::cmp::min(page_end, vaddr + filesz as u64);
        if copy_lo < copy_hi {
            let file_off = offset + (copy_lo - vaddr) as usize;
            let len = (copy_hi - copy_lo) as usize;
            let dst_off = (copy_lo - page_addr) as usize;
            if file_off + len > image.len() {
                return Err("segment file range out of bounds");
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    image[file_off..].as_ptr(),
                    dst.add(dst_off),
                    len,
                );
            }
        }

        unsafe {
            mapper
                .map_to_with_table_flags(page, frame, page_flags, parent_flags, frame_allocator)
                .map_err(|_| "map_to failed for ELF page")?
                .flush();
        }
        crate::memory::make_user_path(page_addr);
        pages.push(page_addr);
        page_addr += 4096;
    }
    x86_64::instructions::tlb::flush_all();
    Ok(())
}
