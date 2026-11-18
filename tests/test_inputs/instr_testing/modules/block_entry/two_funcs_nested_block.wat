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
    block $hi
      ;; << i32.const 12
      ;; << drop
      block ;; label = @2
        ;; << i32.const 12
        ;; << drop
        br $hi
        i32.const 1
        i32.const 2
        i32.add
        drop
      end
    end
  )
  (func (;2;) (type 0)
    (local i32)
    block $hi
      ;; << i32.const 12
      ;; << drop
      block ;; label = @2
        ;; << i32.const 12
        ;; << drop
        br $hi
        i32.const 1
        i32.const 2
        i32.add
        drop
      end
    end
    block $hi
      ;; << i32.const 12
      ;; << drop
      i32.const 0
      if ;; label = @2
        ;; << i32.const 56
        ;; << drop
        br $hi
        i32.const 1
        i32.const 2
        i32.add
        drop
      else
        ;; << i32.const 78
        ;; << drop
        i32.const 1
        i32.const 2
        i32.sub
        drop
      end
    end
  )
  (memory (;0;) 1)
  (export "add" (func $add))
  (export "memory" (memory 0))
)
