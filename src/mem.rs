mod allocator;

use crate::mem::allocator::VMAllocator;
pub use allocator::{HeapAllocator, VMAllocError};

pub type GuestUSize = u32;

pub type GuestISize = i32;

/// [std::mem::size_of], but returning a [GuestUSize].
pub const fn guest_size_of<T: Sized>() -> GuestUSize {
    assert!(std::mem::size_of::<T>() <= u32::MAX as usize);
    std::mem::size_of::<T>() as u32
}

type VAddr = GuestUSize;

#[repr(transparent)]
pub struct Ptr<T, const MUT: bool>(VAddr, std::marker::PhantomData<T>);

impl<T, const MUT: bool> Clone for Ptr<T, MUT> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, const MUT: bool> Copy for Ptr<T, MUT> {}

impl<T, const MUT: bool> PartialEq for Ptr<T, MUT> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T, const MUT: bool> Eq for Ptr<T, MUT> {}
impl<T, const MUT: bool> std::hash::Hash for Ptr<T, MUT> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Constant guest pointer type (like Rust's `*const T`).
pub type ConstPtr<T> = Ptr<T, false>;
/// Mutable guest pointer type (like Rust's `*mut T`).
pub type MutPtr<T> = Ptr<T, true>;

#[allow(dead_code)]
/// Constant guest pointer-to-void type (like C's `const void *`)
pub type ConstVoidPtr = ConstPtr<std::ffi::c_void>;
/// Mutable guest pointer-to-void type (like C's `void *`)
pub type MutVoidPtr = MutPtr<std::ffi::c_void>;

impl<T, const MUT: bool> Ptr<T, MUT> {
    pub const fn null() -> Self {
        Ptr(0, std::marker::PhantomData)
    }

    pub fn to_bits(self) -> VAddr {
        self.0
    }
    pub fn from_bits(bits: VAddr) -> Self {
        Ptr(bits, std::marker::PhantomData)
    }

    pub fn cast<U>(self) -> Ptr<U, MUT> {
        Ptr::<U, MUT>::from_bits(self.to_bits())
    }

    pub fn cast_void(self) -> Ptr<std::ffi::c_void, MUT> {
        self.cast()
    }

    pub fn is_null(self) -> bool {
        self.to_bits() == 0
    }
}

impl<T> ConstPtr<T> {
    pub fn cast_mut(self) -> MutPtr<T> {
        Ptr::from_bits(self.to_bits())
    }
}

impl<T> MutPtr<T> {
    pub fn cast_const(self) -> ConstPtr<T> {
        Ptr::from_bits(self.to_bits())
    }
}

impl<T, const MUT: bool> Default for Ptr<T, MUT> {
    fn default() -> Self {
        Self::null()
    }
}

impl<T, const MUT: bool> std::fmt::Debug for Ptr<T, MUT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.to_bits())
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct GuestVar<T> {
    ptr: MutPtr<T>,
}

impl<T> Clone for GuestVar<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for GuestVar<T> {}

impl<T> Default for GuestVar<T> {
    fn default() -> Self {
        Self {
            ptr: MutPtr::null(),
        }
    }
}

impl<T> GuestVar<T> {
    pub fn new(mem: &mut Memory, value: T) -> Self
    where
        T: SafeWrite,
    {
        Self {
            ptr: mem.alloc_and_write(value),
        }
    }

    pub fn ptr(&self) -> MutPtr<T> {
        self.ptr
    }

    pub fn to_bits(&self) -> GuestUSize {
        self.ptr().to_bits()
    }

    pub fn get(&self, mem: &Memory) -> T
    where
        T: SafeRead,
    {
        mem.read(self.ptr)
    }

    pub fn set(&self, mem: &mut Memory, value: T)
    where
        T: SafeWrite,
    {
        mem.write(self.ptr, value);
    }

    pub fn update(&self, mem: &mut Memory, update: impl FnOnce(T) -> T) -> T
    where
        T: Copy + SafeRead + SafeWrite,
    {
        let old = self.get(mem);
        self.set(mem, update(old));
        old
    }
}

// C-like pointer arithmetic
impl<T, const MUT: bool> std::ops::Add<GuestUSize> for Ptr<T, MUT> {
    type Output = Self;

    fn add(self, other: GuestUSize) -> Self {
        let size: GuestUSize = std::mem::size_of::<T>().try_into().unwrap();
        Self::from_bits(
            self.to_bits()
                .checked_add(other.checked_mul(size).unwrap())
                .unwrap(),
        )
    }
}

impl<T, const MUT: bool> std::ops::Sub<GuestUSize> for Ptr<T, MUT> {
    type Output = Self;
    fn sub(self, other: GuestUSize) -> Self {
        let size: GuestUSize = std::mem::size_of::<T>().try_into().unwrap();
        Self::from_bits(
            self.to_bits()
                .checked_sub(other.checked_mul(size).unwrap())
                .unwrap(),
        )
    }
}

impl<T, const MUT: bool> std::ops::AddAssign<GuestUSize> for Ptr<T, MUT> {
    fn add_assign(&mut self, rhs: GuestUSize) {
        *self = *self + rhs;
    }
}

pub trait SafeRead: Sized {}
impl SafeRead for i8 {}
impl SafeRead for u8 {}
impl SafeRead for i16 {}
impl SafeRead for u16 {}
impl SafeRead for i32 {}
impl SafeRead for u32 {}
impl SafeRead for i64 {}
impl SafeRead for u64 {}
impl SafeRead for f32 {}
impl SafeRead for f64 {}
impl<T, const MUT: bool> SafeRead for Ptr<T, MUT> {}

pub trait SafeWrite: Sized {}
impl<T: SafeRead> SafeWrite for T {}

pub const LINEAR_MEMORY_SIZE: GuestUSize = 8 * 1024 * 1024;
type Bytes = [u8; LINEAR_MEMORY_SIZE as usize];
pub const PAGE_SIZE: GuestUSize = 4096;

pub struct Memory {
    bytes: *mut Bytes,
    heap_allocator: Option<HeapAllocator>,
    vm_allocator: VMAllocator,
}

impl Drop for Memory {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::new::<Bytes>();
        unsafe {
            std::alloc::dealloc(self.bytes as *mut _, layout);
        }
    }
}

impl Memory {
    pub const NULL_PAGE_SIZE: VAddr = 0x1000;
    pub const STACK_SIZE: GuestUSize = 1024 * 1024;
    pub const STACK_HIGH_END: VAddr = LINEAR_MEMORY_SIZE;
    pub const STACK_LOW_END: VAddr = Self::STACK_HIGH_END - Self::STACK_SIZE;

    pub fn new() -> Memory {
        let layout = std::alloc::Layout::new::<Bytes>();
        let bytes = unsafe { std::alloc::alloc_zeroed(layout) as *mut Bytes };
        let vm_allocator = VMAllocator::new(0, Self::STACK_LOW_END);
        Memory {
            bytes,
            vm_allocator,
            heap_allocator: None,
        }
    }

    pub fn create_heap(&mut self, size: GuestUSize) -> HeapAllocator {
        HeapAllocator::new(&mut self.vm_allocator, size)
    }

    pub fn destroy_heap(&mut self, heap: HeapAllocator) {
        for chunk in heap.into_vm_chunks() {
            self.vm_free(Ptr::from_bits(chunk.base), chunk.size.get());
        }
    }

    fn bytes(&self) -> &Bytes {
        unsafe { &*self.bytes }
    }
    fn bytes_mut(&mut self) -> &mut Bytes {
        unsafe { &mut *self.bytes }
    }

    #[cold]
    fn bounds_check_fail(at: u32, size: u32) -> ! {
        panic!(
            "Guest memory access out of bounds at {:#x} ({:#x} bytes)",
            at, size
        )
    }

    pub fn bytes_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>, count: GuestUSize) -> &[u8] {
        self.bytes()
            .get(ptr.to_bits() as usize..)
            .and_then(|bytes| bytes.get(..count as usize))
            .unwrap_or_else(|| Self::bounds_check_fail(ptr.to_bits(), count))
    }
    pub fn bytes_at_mut(&mut self, ptr: MutPtr<u8>, count: GuestUSize) -> &mut [u8] {
        self.bytes_mut()
            .get_mut(ptr.to_bits() as usize..)
            .and_then(|bytes| bytes.get_mut(..count as usize))
            .unwrap_or_else(|| Self::bounds_check_fail(ptr.to_bits(), count))
    }

    pub fn alloc(&mut self, size: GuestUSize) -> MutVoidPtr {
        self.alloc_in_heap(None, size)
    }

    /// Allocate `size` bytes in `heap`.
    pub fn alloc_in_heap(
        &mut self,
        heap: Option<&mut HeapAllocator>,
        size: GuestUSize,
    ) -> MutVoidPtr {
        let (vm, heap) = self.allocators_mut(heap);
        let alloc = match heap.alloc(vm, size) {
            None => {
                panic!("Could not find large enough chunk to allocate {size:#x} bytes")
            }
            Some(alloc) => alloc,
        };
        let ptr = Ptr::from_bits(alloc.base);
        log_dbg!("Allocated {:?} ({:#x} bytes)", ptr, size);
        ptr
    }

    /// Get size of allocation at `ptr` in the default heap.
    pub fn malloc_size(&mut self, ptr: ConstVoidPtr) -> GuestUSize {
        self.malloc_size_in_heap(None, ptr)
    }

    /// Get size of allocation at `ptr` in `heap`.
    pub fn malloc_size_in_heap(
        &mut self,
        heap: Option<&mut HeapAllocator>,
        ptr: ConstVoidPtr,
    ) -> GuestUSize {
        let (_, heap) = self.allocators_mut(heap);
        heap.find_allocated_size(ptr.to_bits())
    }

    /// Resize allocation at `old_ptr` to `size` bytes in the default heap.
    pub fn realloc(&mut self, old_ptr: MutVoidPtr, size: GuestUSize) -> MutVoidPtr {
        self.realloc_in_heap(None, old_ptr, size)
    }

    /// Resize allocation at `old_ptr` to `size` bytes in `heap`.
    pub fn realloc_in_heap(
        &mut self,
        mut heap: Option<&mut HeapAllocator>,
        old_ptr: MutVoidPtr,
        size: GuestUSize,
    ) -> MutVoidPtr {
        if old_ptr.is_null() {
            return self.alloc_in_heap(heap, size);
        }
        // TODO: for a moment we always assume that we do not have enough size
        //       to realloc inplace
        let old_size = self.malloc_size_in_heap(heap.as_deref_mut(), old_ptr.cast_const());
        if old_size >= size {
            return old_ptr;
        }
        let new_ptr = self.alloc_in_heap(heap.as_deref_mut(), size);
        self.memmove(new_ptr, old_ptr.cast_const(), old_size);
        self.free_in_heap(heap, old_ptr);
        new_ptr
    }

    /// Allocate `size` bytes using the virtual memory allocator.
    /// All allocations are page aligned, page sized and zeroed.
    pub fn vm_alloc(
        &mut self,
        address: Option<VAddr>,
        size: GuestUSize,
    ) -> Result<MutVoidPtr, VMAllocError> {
        let allocation = self.vm_allocator.allocate(address, size)?;

        let ptr = Ptr::from_bits(allocation.base);

        // VM allocations are always 0 initialized.
        // TODO: Can this be done with vm_advise/equivalents
        self.bytes_at_mut(ptr.cast(), allocation.size.get()).fill(0);

        Ok(ptr)
    }

    /// Free allocations made with non vm prefixed `alloc` methods on
    /// this type in the default heap.
    pub fn free(&mut self, ptr: MutVoidPtr) {
        self.free_in_heap(None, ptr);
    }

    /// Free an allocation made with one of the `alloc` methods in `heap`.
    pub fn free_in_heap(&mut self, heap: Option<&mut HeapAllocator>, ptr: MutVoidPtr) {
        let (vm, heap) = self.allocators_mut(heap);
        let size = heap.free(vm, ptr.to_bits());

        if size > HeapAllocator::HEAP_ALLOCATION_THRESHOLD {
            // VM allocations are always 0 initialized.
            // TODO: Can this be done with vm_advise/equivalents
            self.bytes_at_mut(ptr.cast(), size).fill(0);
        }

        log_dbg!("Freed {:?} ({:#x} bytes)", ptr, size);
    }

    /// Free an allocation made with `vm_alloc` or `reserve`. All allocations
    /// within the provided range are freed.
    pub fn vm_free(&mut self, ptr: MutVoidPtr, size: GuestUSize) {
        let freed = self.vm_allocator.deallocate(ptr.to_bits(), size);
        // VM allocations are always 0 initialized.
        // TODO: Can this be done with vm_advise/equivalents
        self.bytes_at_mut(Ptr::from_bits(freed.base), freed.size.get())
            .fill(0);
    }

    pub fn calloc(&mut self, size: GuestUSize) -> MutVoidPtr {
        let ptr = self.alloc(size);
        self.bytes_at_mut(ptr.cast(), size).fill(0);
        ptr
    }

    pub fn read<T, const MUT: bool>(&self, ptr: Ptr<T, MUT>) -> T
    where
        T: SafeRead,
    {
        let size = std::mem::size_of::<T>().try_into().unwrap();
        let slice = self.bytes_at(ptr.cast(), size);
        let ptr: *const T = slice.as_ptr().cast();
        unsafe { ptr.read_unaligned() }
    }
    pub fn write<T>(&mut self, ptr: MutPtr<T>, value: T)
    where
        T: SafeWrite,
    {
        let size = std::mem::size_of::<T>().try_into().unwrap();
        assert!(size > 0);
        let slice = self.bytes_at_mut(ptr.cast(), size);
        let ptr: *mut T = slice.as_mut_ptr().cast();
        unsafe { ptr.write_unaligned(value) }
    }

    /// C-style `memmove`.
    pub fn memmove(&mut self, dest: MutVoidPtr, src: ConstVoidPtr, size: GuestUSize) {
        let src = src.to_bits() as usize;
        let dest = dest.to_bits() as usize;
        let size = size as usize;
        self.bytes_mut()
            .copy_within(src..src.checked_add(size).unwrap(), dest)
    }

    pub fn alloc_and_write<T>(&mut self, value: T) -> MutPtr<T>
    where
        T: SafeWrite,
    {
        let size = std::mem::size_of::<T>().try_into().unwrap();
        let ptr = self.alloc(size).cast();
        self.write(ptr, value);
        ptr
    }

    pub fn alloc_and_write_cstr(&mut self, str_bytes: &[u8]) -> MutPtr<u8> {
        let len = str_bytes.len().try_into().unwrap();
        let ptr = self.alloc(len + 1).cast();
        self.bytes_at_mut(ptr, len).copy_from_slice(str_bytes);
        self.write(ptr + len, b'\0');
        ptr
    }

    /// Get a C string (null terminated) as a slice.
    pub fn cstr_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> &[u8] {
        let mut len = 0;
        while self.read(ptr + len) != b'\0' {
            len += 1;
        }
        self.bytes_at(ptr, len)
    }

    /// Get a C string (null-terminated) as a string slice, if it is valid
    /// UTF-8, otherwise returning a byte slice. The null terminator is not
    /// included in the slice.
    pub fn cstr_at_utf8<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> Result<&str, &[u8]> {
        let bytes = self.cstr_at(ptr);
        std::str::from_utf8(bytes).map_err(|_| bytes)
    }

    pub fn wstr_len_bytes_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> GuestUSize {
        let mut len = 0;
        while ((u16::from(self.read::<u8, MUT>(ptr + len)) << 8)
            | u16::from(self.read::<u8, MUT>(ptr + len + 1)))
            != 0
        {
            len += 2;
        }
        len
    }

    pub fn reserve(&mut self, base: VAddr, size: GuestUSize) {
        self.vm_allocator.allocate(Some(base), size).unwrap();
    }

    /// Returns a mutable references to the vm allocator and either the
    /// provided heap or the default heap if no heap is provided.
    fn allocators_mut<'a>(
        &'a mut self,
        heap: Option<&'a mut HeapAllocator>,
    ) -> (&'a mut VMAllocator, &'a mut HeapAllocator) {
        let vm = &mut self.vm_allocator;
        let heap = heap.unwrap_or_else(|| {
            self.heap_allocator
                .get_or_insert_with(|| HeapAllocator::new(vm, HeapAllocator::HEAP_CHUNK_SIZE))
        });

        (vm, heap)
    }
}
