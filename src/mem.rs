mod allocator;

pub type GuestUSize = u32;

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

type Bytes = [u8; 1 << 32];

pub struct Memory {
    bytes: *mut Bytes,
    allocator: allocator::Allocator,
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
    pub const STACK_LOW_END: VAddr = 0u32.wrapping_sub(Self::STACK_SIZE);

    pub fn new() -> Memory {
        let layout = std::alloc::Layout::new::<Bytes>();
        let bytes = unsafe { std::alloc::alloc_zeroed(layout) as *mut Bytes };
        let allocator = allocator::Allocator::new();
        Memory { bytes, allocator }
    }

    fn bytes(&self) -> &Bytes {
        unsafe { &*self.bytes }
    }
    fn bytes_mut(&mut self) -> &mut Bytes {
        unsafe { &mut *self.bytes }
    }

    #[cold]
    fn null_check_fail(at: u32, size: u32) {
        panic!(
            "Attempted null-page access at {:#x} ({:#x} bytes)",
            at, size
        )
    }

    pub fn bytes_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>, count: GuestUSize) -> &[u8] {
        if ptr.to_bits() < Self::NULL_PAGE_SIZE {
            Self::null_check_fail(ptr.to_bits(), count)
        }
        &self.bytes()[ptr.to_bits() as usize..][..count as usize]
    }
    pub fn bytes_at_mut(&mut self, ptr: MutPtr<u8>, count: GuestUSize) -> &mut [u8] {
        if ptr.to_bits() < Self::NULL_PAGE_SIZE {
            Self::null_check_fail(ptr.to_bits(), count)
        }
        &mut self.bytes_mut()[ptr.to_bits() as usize..][..count as usize]
    }

    pub fn alloc(&mut self, size: GuestUSize) -> MutVoidPtr {
        let ptr = Ptr::from_bits(self.allocator.alloc(size));
        log_dbg!("Allocated {:?} ({:#x} bytes)", ptr, size);
        ptr
    }

    pub fn free(&mut self, ptr: MutVoidPtr) {
        let size = self.allocator.free(ptr.to_bits());
        self.bytes_at_mut(ptr.cast(), size).fill(0);
        log_dbg!("Mem: freed {:?} ({:#x} bytes)", ptr, size);
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

    /// Get a C string (null terminated) as a string reference, panicking if it
    /// is not UTF-8.
    pub fn cstr_at_utf8<const MUT: bool>(&self, ptr: Ptr<u8, MUT>) -> &str {
        std::str::from_utf8(self.cstr_at(ptr)).unwrap()
    }

    pub fn reserve(&mut self, base: VAddr, size: GuestUSize) {
        self.allocator.reserve(allocator::Chunk::new(base, size));
    }
}
