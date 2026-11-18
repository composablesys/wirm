(module
  (type (;0;) (func))
  (type (;1;) (func (result i32)))
  (type (;2;) (func (param i32 i32)))
  (type (;3;) (func (param i32 i32 i32 i32) (result i32)))
  (type (;4;) (func (param i32 i32 i32 i32 i32 i32 i32 i32)))
  (type (;5;) (func (param i32 i32) (result i32)))
  (import "bogus" "hi" (func (;0;) (type 2)))
  (func $add (;1;) (type 2) (param i32 i32)
    (local f32)
    block ;; label = @1
      br 0 (;@1;)
      i32.const 1
      i32.const 2
      i32.add
      drop
      ;; << i32.const 12
      ;; << drop
    end
  )
  (func (;2;) (type 0)
    (local i32)
    loop ;; label = @1
      br 0 (;@1;)
      i32.const 1
      i32.const 2
      i32.add
      drop
      ;; << i32.const 34
      ;; << drop
    end
    i32.const 1
    if ;; label = @1
      br 0 (;@1;)
      i32.const 1
      i32.const 2
      i32.add
      drop
      ;; << i32.const 56
      ;; << drop
    end
    i32.const 0
    if ;; label = @1
      br 0 (;@1;)
      i32.const 1
      i32.const 2
      i32.add
      drop
      ;; << i32.const 56
      ;; << drop
    else
      i32.const 1
      i32.const 2
      i32.add
      drop
      ;; << i32.const 78
      ;; << drop
    end
  )
  (memory (;0;) 1)
  (export "add" (func $add))
  (export "memory" (memory 0))
)
