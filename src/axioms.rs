/// Simple axiomatic proof of safety.
///
/// The theory of safety can be summarized by viewing each operation on
/// references in terms of how they alter the number of different kinds of
/// references. We denote the state of the world as a four-tuple per reference.
///
/// Many of the axioms are presented in pairs where the postconditions of one
/// is the preconditions of the other and vice versa. Some are unpaired.
#[allow(dead_code)]
#[must_use]
pub struct Axioms
{
    /// A number of owned references.
    pub owned: usize,

    /// A number of unique references.
    pub unique: usize,

    /// A number of valid weak references.
    pub valid_weak: usize,

    /// A number of invalid weak references from earlier allocations.
    pub invalid_weak: usize,
}

#[allow(dead_code)]
impl Axioms
{
    /// Unallocated memory has no references.
    ///
    /// ```notest
    /// Self {
    ///     owned: 0,
    ///     unique: 0,
    ///     valid_weak: 0,
    ///     invalid_weak: 0,
    /// }
    /// ```
    ///
    /// Sequencing property:
    ///
    /// ```
    /// genref::Axioms::mmap().segfault() 
    /// ```
    pub fn mmap() -> Self
    {
        Self {
            owned: 0,
            unique: 0,
            valid_weak: 0,
            invalid_weak: 0,
        }
    }

    /// Memory segmentation prevents cross-referencing.
    ///
    /// ```notest
    /// assert_eq!(
    ///     self.owned + self.unique + self.valid_weak + self.invalid_weak,
    ///     0
    /// );
    /// std::mem::drop(self);
    /// ```
    pub fn segfault(self)
    {
        assert_eq!(
            self.owned + self.unique + self.valid_weak + self.invalid_weak,
            0
        );
        std::mem::drop(self);
    }

    /// Program exit does not care about the number of references.
    ///
    /// ```notest
    /// std::mem::forget(self);
    /// ```
    pub fn leak(self) { std::mem::forget(self); }

    /// Allocating fresh creates precisely one reference to new memory that is
    /// guaranteed to be unique.
    ///
    /// ```notest
    /// assert_eq!(
    ///     self.owned + self.unique + self.valid_weak + self.invalid_weak,
    ///     0
    /// );
    /// self.unique += 1;
    /// self
    /// ```
    ///
    /// Sequencing property:
    /// ```
    /// genref::Axioms::mmap().malloc().free().segfault(); 
    /// ```
    pub fn malloc(mut self) -> Self
    {
        assert_eq!(
            self.owned + self.unique + self.valid_weak + self.invalid_weak,
            0
        );
        self.unique += 1;
        self
    }

    /// An allocation with precisely one unique reference can be safely
    /// deallocated.
    ///
    /// ```notest
    /// assert_eq!(self.unique, 1);
    /// assert_eq!(self.owned + self.valid_weak + self.invalid_weak, 0);
    /// ```
    pub fn free(mut self) -> Self
    {
        assert_eq!(self.unique, 1);
        assert_eq!(self.owned + self.valid_weak + self.invalid_weak, 0);

        self.unique -= 1;
        self
    }

    /// It is only safe to re-use an allocation that has no valid references.
    ///
    /// ```notest
    /// assert_eq!(self.owned + self.unique + self.valid_weak, 0);
    /// self.unique = 1;
    /// self
    /// ```
    ///
    /// Sequencing:
    /// ```
    /// genref::Axioms::mmap()
    ///     .malloc()
    ///     .deinit()
    ///     .reinit()
    ///     .free()
    ///     .segfault();
    /// ```
    pub fn reinit(mut self) -> Self
    {
        assert_eq!(self.owned + self.unique + self.valid_weak, 0);
        self.unique = 1;
        self
    }

    /// An object under unique reference can be safely dropped.
    ///
    /// ```notest
    /// assert_eq!(self.unique, 1);
    /// assert_eq!(self.owned + self.valid_weak, 0);
    ///
    /// self.unique = 0;
    /// self
    /// ```
    pub fn deinit(mut self) -> Self
    {
        assert_eq!(self.unique, 1);
        assert_eq!(self.owned + self.valid_weak, 0);

        self.unique = 0;
        self
    }

    /// A unique reference can decay into a merely owned reference for future
    /// aliasing.
    ///
    /// ```notest
    /// assert_eq!(self.unique, 1);
    /// assert_eq!(self.owned + self.valid_weak, 0);
    ///
    /// self.unique -= 1;
    /// self.owned += 1;
    /// self
    /// ```
    pub fn decay(mut self) -> Self
    {
        assert_eq!(self.unique, 1);
        assert_eq!(self.owned + self.valid_weak, 0);

        self.unique -= 1;
        self.owned += 1;
        self
    }

    /// An owned reference with no aliases can be promoted to unique for
    /// transfer across threads.
    ///
    /// ```notest
    /// assert_eq!(self.owned, 1);
    /// assert_eq!(self.unique + self.valid_weak, 0);
    ///
    /// self.owned -= 1;
    /// self.unique += 1;
    /// self
    /// ```
    ///
    /// Sequencing:
    /// ```
    /// genref::Axioms::mmap()
    ///     .malloc()
    ///     .decay()
    ///     .promote()
    ///     .free()
    ///     .segfault();
    /// ```
    pub fn promote(mut self) -> Self
    {
        assert_eq!(self.owned, 1);
        assert_eq!(self.unique + self.valid_weak, 0);

        self.owned -= 1;
        self.unique += 1;
        self
    }

    /// An owned reference can be aliased.
    ///
    /// ```notest
    /// assert_eq!(self.unique, 0);
    /// assert_eq!(self.owned, 1);
    /// self.valid_weak += 1;
    /// self
    /// ```
    ///
    /// This does not have a converse; there is no reliable
    /// way to dispose of or track weak references since they are `Copy`.
    pub fn alias(mut self, n: usize) -> Self
    {
        assert_eq!(self.unique, 0);
        assert_eq!(self.owned, 1);
        self.valid_weak += n;
        self
    }

    /// By incrementing the generation counter all weak references
    /// can be invalidated at once.
    ///
    /// ```notest
    /// self.invalid_weak += self.valid_weak;
    /// self.valid_weak = 0;
    /// self
    /// ```
    ///
    /// All axioms are weakly increasing in `self.invalid_weak`.
    pub fn invalidate(mut self) -> Self
    {
        self.invalid_weak += self.valid_weak;
        self.valid_weak = 0;
        self
    }

    /// With these axioms we can show that an owned reference
    /// can be safely dropped by bumping the generation, the main referential
    /// safety claim:
    ///
    /// ```notest
    /// Self::mmap()
    ///     .malloc()
    ///     .decay()
    ///     .alias(100)
    ///     .invalidate()
    ///     .promote()
    ///     .deinit()
    ///     .leak()
    /// ```
    ///
    /// Proof: the following doctest passes
    ///
    /// ```
    /// genref::Axioms::drop_owned() 
    /// ```
    pub fn drop_owned()
    {
        Self::mmap()
            .malloc()
            .decay()
            .alias(100)
            .invalidate()
            .promote()
            .deinit()
            .leak()
    }
}
