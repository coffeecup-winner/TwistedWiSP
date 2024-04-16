#[derive(Debug, Hash, PartialEq, Eq)]
pub struct VarRef(pub u32);

#[derive(Debug)]
pub enum Instruction {
    LoadPrev(VarRef),
    StoreNext(VarRef),
}
