use crate::{
    Py, PyObject, PyObjectRef, PyResult, TryFromObject, VirtualMachine,
    builtins::{PyIntRef, PyTuple},
    convert::TryFromBorrowedObject,
    function::OptionalOption,
};
use num_traits::{cast::ToPrimitive, sign::Signed};

#[derive(FromArgs)]
pub struct SplitArgs<T: TryFromObject> {
    #[pyarg(any, default)]
    sep: Option<T>,
    #[pyarg(any, default = -1)]
    maxsplit: isize,
}

#[derive(FromArgs)]
pub struct SplitLinesArgs {
    #[pyarg(any, default = false)]
    pub keepends: bool,
}

#[derive(FromArgs)]
pub struct ExpandTabsArgs {
    #[pyarg(any, default = 8)]
    tabsize: isize,
}

impl ExpandTabsArgs {
    pub fn tabsize(&self) -> usize {
        self.tabsize.to_usize().unwrap_or(0)
    }
}

#[derive(FromArgs)]
pub struct StartsEndsWithArgs {
    #[pyarg(positional)]
    affix: PyObjectRef,
    #[pyarg(positional, default)]
    start: Option<PyIntRef>,
    #[pyarg(positional, default)]
    end: Option<PyIntRef>,
}

impl StartsEndsWithArgs {
    pub fn get_value(self, len: usize) -> (PyObjectRef, Option<std::ops::Range<usize>>) {
        let range = if self.start.is_some() || self.end.is_some() {
            Some(adjust_indices(self.start, self.end, len))
        } else {
            None
        };
        (self.affix, range)
    }

    #[inline]
    pub fn prepare<S, F>(self, s: &S, len: usize, substr: F) -> Option<(PyObjectRef, &S)>
    where
        S: ?Sized + AnyStr,
        F: Fn(&S, std::ops::Range<usize>) -> &S,
    {
        let (affix, range) = self.get_value(len);
        let substr = if let Some(range) = range {
            if !range.is_normal() {
                return None;
            }
            substr(s, range)
        } else {
            s
        };
        Some((affix, substr))
    }
}

fn saturate_to_isize(py_int: PyIntRef) -> isize {
    let big = py_int.as_bigint();
    big.to_isize().unwrap_or_else(|| {
        if big.is_negative() {
            isize::MIN
        } else {
            isize::MAX
        }
    })
}

// help get optional string indices
pub fn adjust_indices(
    start: Option<PyIntRef>,
    end: Option<PyIntRef>,
    len: usize,
) -> std::ops::Range<usize> {
    let mut start = start.map_or(0, saturate_to_isize);
    let mut end = end.map_or(len as isize, saturate_to_isize);
    if end > len as isize {
        end = len as isize;
    } else if end < 0 {
        end += len as isize;
        if end < 0 {
            end = 0;
        }
    }
    if start < 0 {
        start += len as isize;
        if start < 0 {
            start = 0;
        }
    }
    start as usize..end as usize
}

pub trait StringRange {
    fn is_normal(&self) -> bool;
}

impl StringRange for std::ops::Range<usize> {
    fn is_normal(&self) -> bool {
        self.start <= self.end
    }
}

pub trait AnyStrWrapper<S: AnyStr + ?Sized> {
    fn as_ref(&self) -> Option<&S>;
    fn is_empty(&self) -> bool;
}

pub trait AnyStrContainer<S>
where
    S: ?Sized,
{
    fn new() -> Self;
    fn with_capacity(capacity: usize) -> Self;
    fn push_str(&mut self, s: &S);
}

pub trait AnyChar: Copy {
    fn is_lowercase(self) -> bool;
    fn is_uppercase(self) -> bool;
    fn bytes_len(self) -> usize;
}

pub trait AnyStr {
    type Char: AnyChar;
    type Container: AnyStrContainer<Self> + Extend<Self::Char>;

    fn to_container(&self) -> Self::Container;
    fn as_bytes(&self) -> &[u8];
    fn elements(&self) -> impl Iterator<Item = Self::Char>;
    fn get_bytes(&self, range: std::ops::Range<usize>) -> &Self;
    // FIXME: get_chars is expensive for str
    fn get_chars(&self, range: std::ops::Range<usize>) -> &Self;
    fn bytes_len(&self) -> usize;
    // NOTE: str::chars().count() consumes the O(n) time. But pystr::char_len does cache.
    //       So using chars_len directly is too expensive and the below method shouldn't be implemented.
    // fn chars_len(&self) -> usize;
    fn is_empty(&self) -> bool;

    fn py_add(&self, other: &Self) -> Self::Container {
        let mut new = Self::Container::with_capacity(self.bytes_len() + other.bytes_len());
        new.push_str(self);
        new.push_str(other);
        new
    }

    fn py_split<T, SP, SN, SW>(
        &self,
        args: SplitArgs<T>,
        vm: &VirtualMachine,
        full_obj: impl FnOnce() -> PyObjectRef,
        split: SP,
        splitn: SN,
        split_whitespace: SW,
    ) -> PyResult<Vec<PyObjectRef>>
    where
        T: TryFromObject + AnyStrWrapper<Self>,
        SP: Fn(&Self, &Self, &VirtualMachine) -> Vec<PyObjectRef>,
        SN: Fn(&Self, &Self, usize, &VirtualMachine) -> Vec<PyObjectRef>,
        SW: Fn(&Self, isize, &VirtualMachine) -> Vec<PyObjectRef>,
    {
        if args.sep.as_ref().is_some_and(|sep| sep.is_empty()) {
            return Err(vm.new_value_error("empty separator"));
        }
        let splits = if let Some(pattern) = args.sep {
            let Some(pattern) = pattern.as_ref() else {
                return Ok(vec![full_obj()]);
            };
            if args.maxsplit < 0 {
                split(self, pattern, vm)
            } else {
                splitn(self, pattern, (args.maxsplit + 1) as usize, vm)
            }
        } else {
            split_whitespace(self, args.maxsplit, vm)
        };
        Ok(splits)
    }
    fn py_split_whitespace<F>(&self, maxsplit: isize, convert: F) -> Vec<PyObjectRef>
    where
        F: Fn(&Self) -> PyObjectRef;
    fn py_rsplit_whitespace<F>(&self, maxsplit: isize, convert: F) -> Vec<PyObjectRef>
    where
        F: Fn(&Self) -> PyObjectRef;

    #[inline]
    fn py_starts_ends_with<'a, T, F>(
        &self,
        affix: &'a PyObject,
        func_name: &str,
        py_type_name: &str,
        func: F,
        vm: &VirtualMachine,
    ) -> PyResult<bool>
    where
        T: TryFromBorrowedObject<'a>,
        F: Fn(&Self, T) -> bool,
    {
        single_or_tuple_any(
            affix,
            &|s: T| Ok(func(self, s)),
            &|o| {
                format!(
                    "{} first arg must be {} or a tuple of {}, not {}",
                    func_name,
                    py_type_name,
                    py_type_name,
                    o.class(),
                )
            },
            vm,
        )
    }

    #[inline]
    fn py_strip<'a, S, FC, FD>(
        &'a self,
        chars: OptionalOption<S>,
        func_chars: FC,
        func_default: FD,
    ) -> &'a Self
    where
        S: AnyStrWrapper<Self>,
        FC: Fn(&'a Self, &Self) -> &'a Self,
        FD: Fn(&'a Self) -> &'a Self,
    {
        let chars = chars.flatten();
        match chars {
            Some(chars) => {
                if let Some(chars) = chars.as_ref() {
                    func_chars(self, chars)
                } else {
                    self
                }
            }
            None => func_default(self),
        }
    }

    #[inline]
    fn py_find<F>(&self, needle: &Self, range: std::ops::Range<usize>, find: F) -> Option<usize>
    where
        F: Fn(&Self, &Self) -> Option<usize>,
    {
        if range.is_normal() {
            let start = range.start;
            let index = find(self.get_chars(range), needle)?;
            Some(start + index)
        } else {
            None
        }
    }

    #[inline]
    fn py_count<F>(&self, needle: &Self, range: std::ops::Range<usize>, count: F) -> usize
    where
        F: Fn(&Self, &Self) -> usize,
    {
        if range.is_normal() {
            count(self.get_chars(range), needle)
        } else {
            0
        }
    }

    fn py_pad(&self, left: usize, right: usize, fillchar: Self::Char) -> Self::Container {
        let mut u = Self::Container::with_capacity(
            (left + right) * fillchar.bytes_len() + self.bytes_len(),
        );
        u.extend(std::iter::repeat_n(fillchar, left));
        u.push_str(self);
        u.extend(std::iter::repeat_n(fillchar, right));
        u
    }

    fn py_center(&self, width: usize, fillchar: Self::Char, len: usize) -> Self::Container {
        let marg = width - len;
        let left = marg / 2 + (marg & width & 1);
        self.py_pad(left, marg - left, fillchar)
    }

    fn py_ljust(&self, width: usize, fillchar: Self::Char, len: usize) -> Self::Container {
        self.py_pad(0, width - len, fillchar)
    }

    fn py_rjust(&self, width: usize, fillchar: Self::Char, len: usize) -> Self::Container {
        self.py_pad(width - len, 0, fillchar)
    }

    fn py_join(
        &self,
        mut iter: impl std::iter::Iterator<Item = PyResult<impl AnyStrWrapper<Self> + TryFromObject>>,
    ) -> PyResult<Self::Container> {
        let mut joined = if let Some(elem) = iter.next() {
            elem?.as_ref().unwrap().to_container()
        } else {
            return Ok(Self::Container::new());
        };
        for elem in iter {
            let elem = elem?;
            joined.push_str(self);
            joined.push_str(elem.as_ref().unwrap());
        }
        Ok(joined)
    }

    fn py_partition<'a, F, S>(
        &'a self,
        sub: &Self,
        split: F,
        vm: &VirtualMachine,
    ) -> PyResult<(Self::Container, bool, Self::Container)>
    where
        F: Fn() -> S,
        S: std::iter::Iterator<Item = &'a Self>,
    {
        if sub.is_empty() {
            return Err(vm.new_value_error("empty separator"));
        }

        let mut sp = split();
        let front = sp.next().unwrap().to_container();
        let (has_mid, back) = if let Some(back) = sp.next() {
            (true, back.to_container())
        } else {
            (false, Self::Container::new())
        };
        Ok((front, has_mid, back))
    }

    fn py_removeprefix<FC>(&self, prefix: &Self, prefix_len: usize, is_prefix: FC) -> &Self
    where
        FC: Fn(&Self, &Self) -> bool,
    {
        //if self.py_starts_with(prefix) {
        if is_prefix(self, prefix) {
            self.get_bytes(prefix_len..self.bytes_len())
        } else {
            self
        }
    }

    fn py_removesuffix<FC>(&self, suffix: &Self, suffix_len: usize, is_suffix: FC) -> &Self
    where
        FC: Fn(&Self, &Self) -> bool,
    {
        if is_suffix(self, suffix) {
            self.get_bytes(0..self.bytes_len() - suffix_len)
        } else {
            self
        }
    }

    // TODO: remove this function from anystr.
    // See https://github.com/RustPython/RustPython/pull/4709/files#r1141013993
    fn py_bytes_splitlines<FW, W>(&self, options: SplitLinesArgs, into_wrapper: FW) -> Vec<W>
    where
        FW: Fn(&Self) -> W,
    {
        let keep = options.keepends as usize;
        let mut elements = Vec::new();
        let mut last_i = 0;
        let mut enumerated = self.as_bytes().iter().enumerate().peekable();
        while let Some((i, ch)) = enumerated.next() {
            let (end_len, i_diff) = match *ch {
                b'\n' => (keep, 1),
                b'\r' => {
                    let is_rn = enumerated.next_if(|(_, ch)| **ch == b'\n').is_some();
                    if is_rn { (keep + keep, 2) } else { (keep, 1) }
                }
                _ => continue,
            };
            let range = last_i..i + end_len;
            last_i = i + i_diff;
            elements.push(into_wrapper(self.get_bytes(range)));
        }
        if last_i != self.bytes_len() {
            elements.push(into_wrapper(self.get_bytes(last_i..self.bytes_len())));
        }
        elements
    }

    fn py_zfill(&self, width: isize) -> Vec<u8> {
        let width = width.to_usize().unwrap_or(0);
        rustpython_common::str::zfill(self.as_bytes(), width)
    }

    // Unified form of CPython functions:
    //  _Py_bytes_islower
    //  unicode_islower_impl
    fn py_islower(&self) -> bool {
        let mut lower = false;
        for c in self.elements() {
            if c.is_uppercase() {
                return false;
            } else if !lower && c.is_lowercase() {
                lower = true
            }
        }
        lower
    }

    // Unified form of CPython functions:
    //   Py_bytes_isupper
    //  unicode_isupper_impl
    fn py_isupper(&self) -> bool {
        let mut upper = false;
        for c in self.elements() {
            if c.is_lowercase() {
                return false;
            } else if !upper && c.is_uppercase() {
                upper = true
            }
        }
        upper
    }
}

/// Tests that the predicate is True on a single value, or if the value is a tuple a tuple, then
/// test that any of the values contained within the tuples satisfies the predicate. Type parameter
/// T specifies the type that is expected, if the input value is not of that type or a tuple of
/// values of that type, then a TypeError is raised.
pub fn single_or_tuple_any<'a, T, F, M>(
    obj: &'a PyObject,
    predicate: &F,
    message: &M,
    vm: &VirtualMachine,
) -> PyResult<bool>
where
    T: TryFromBorrowedObject<'a>,
    F: Fn(T) -> PyResult<bool>,
    M: Fn(&PyObject) -> String,
{
    match obj.try_to_value::<T>(vm) {
        Ok(single) => (predicate)(single),
        Err(_) => {
            let tuple: &Py<PyTuple> = obj
                .try_to_value(vm)
                .map_err(|_| vm.new_type_error((message)(obj)))?;
            for obj in tuple {
                if single_or_tuple_any(obj, predicate, message, vm)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}
