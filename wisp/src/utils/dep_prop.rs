use std::{
    cell::{Cell, Ref, RefCell},
    rc::{Rc, Weak},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PropertyState {
    Outdated,
    #[default]
    Valid,
}

#[derive(Debug, Clone)]
pub struct DependencyHandle(Weak<Cell<PropertyState>>);

#[derive(Debug, Default)]
struct PropertyTracker {
    state: Rc<Cell<PropertyState>>,
    dependencies: RefCell<Vec<Weak<Cell<PropertyState>>>>,
}

impl PropertyTracker {
    pub fn handle(&self) -> DependencyHandle {
        DependencyHandle(Rc::<_>::downgrade(&self.state))
    }

    pub fn is_valid(&self) -> bool {
        self.state.get() == PropertyState::Valid
    }

    pub fn mark_as_valid(&self) {
        self.state.set(PropertyState::Valid);
    }

    pub fn add_dependent(&self, dependent: DependencyHandle) {
        self.dependencies.borrow_mut().push(dependent.0);
    }

    pub fn invalidate_dependents(&self) {
        let mut dependencies = self.dependencies.borrow_mut();
        let mut i = 0;
        while i < dependencies.len() {
            if let Some(dependency) = dependencies[i].upgrade() {
                // Invalidate the dependency if it still exists.
                dependency.set(PropertyState::Outdated);
                i += 1;
            } else {
                // Remove the dependency if it has been dropped, inserting the last element in its place.
                // Do not increment `i` in this case, as the last element is now at the current index.
                dependencies.swap_remove(i);
            }
        }
    }
}

#[derive(Debug)]
pub struct Property<T> {
    tracker: PropertyTracker,
    value: RefCell<T>,
}

impl<T> Property<T> {
    pub fn new(value: T) -> Self {
        Property {
            tracker: PropertyTracker::default(),
            value: RefCell::new(value),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.tracker.is_valid()
    }

    pub fn handle(&self) -> DependencyHandle {
        self.tracker.handle()
    }

    pub fn get_untracked(&self) -> Ref<T> {
        self.value.borrow()
    }

    pub fn get(&self, dependent: DependencyHandle) -> Ref<T> {
        self.tracker.add_dependent(dependent);
        self.value.borrow()
    }

    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
        self.tracker.invalidate_dependents();
        self.tracker.mark_as_valid();
    }

    pub fn update(&self, f: impl FnOnce(DependencyHandle) -> T) {
        if !self.is_valid() {
            let new_value = f(self.handle());
            self.set(new_value);
        }
    }
}

#[allow(dead_code)]
impl<T> Property<T>
where
    T: PartialEq,
{
    pub fn set_if_changed(&self, value: T) {
        if *self.value.borrow() != value {
            *self.value.borrow_mut() = value;
            self.tracker.invalidate_dependents();
        }
        self.tracker.mark_as_valid();
    }

    pub fn update_if_changed(&mut self, f: impl FnOnce(DependencyHandle) -> T) {
        if !self.is_valid() {
            let new_value = f(self.handle());
            self.set_if_changed(new_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_invalidation() {
        let parent = Property::new(42);
        let child = Property::new(0);

        child.set(*parent.get(child.handle()));

        parent.set(41);

        assert_eq!(true, parent.is_valid());
        assert_eq!(false, child.is_valid());
    }

    #[test]
    fn two_parents() {
        let parent1 = Property::new(42);
        let parent2 = Property::new(42);
        let child = Property::new(0);

        child.set(*parent1.get(child.handle()) + *parent2.get(child.handle()));

        parent1.set(41);

        assert_eq!(true, parent1.is_valid());
        assert_eq!(true, parent2.is_valid());
        assert_eq!(false, child.is_valid());

        child.set(*parent1.get(child.handle()) + *parent2.get(child.handle()));

        parent2.set(41);

        assert_eq!(true, parent1.is_valid());
        assert_eq!(true, parent2.is_valid());
        assert_eq!(false, child.is_valid());
    }

    #[test]
    fn two_children() {
        let parent = Property::new(42);
        let child1 = Property::new(0);
        let child2 = Property::new(0);

        child1.set(*parent.get(child1.handle()));
        child2.set(*parent.get(child2.handle()));

        parent.set(41);

        assert_eq!(true, parent.is_valid());
        assert_eq!(false, child1.is_valid());
        assert_eq!(false, child2.is_valid());
    }

    #[test]
    fn set_if_changed() {
        let parent = Property::new(42);
        let child = Property::new(0);

        child.set(*parent.get(child.handle()));

        parent.set_if_changed(42);

        assert_eq!(true, parent.is_valid());
        assert_eq!(true, child.is_valid());

        parent.set_if_changed(41);

        assert_eq!(true, parent.is_valid());
        assert_eq!(false, child.is_valid());
    }

    #[test]
    fn chain_invalidation_rules() {
        let grandparent = Property::new(42);
        let parent = Property::new(0);
        let child = Property::new(0);

        parent.set(*grandparent.get(parent.handle()) / 2);
        child.set(*parent.get(child.handle()) / 2);

        grandparent.set(41);

        assert_eq!(true, grandparent.is_valid());
        assert_eq!(false, parent.is_valid());
        // No hierarchical tracking
        assert_eq!(true, child.is_valid());

        // Setting it to the same value it used to be
        parent.set_if_changed(42 / 2);

        assert_eq!(true, grandparent.is_valid());
        assert_eq!(true, parent.is_valid());
        // Child was never invalidated
        assert_eq!(true, child.is_valid());
    }
}
