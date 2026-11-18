(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    ;; << (local i32)
    block $hi
      block ;; label = @2
        i32.const 1
        ;; << i32.const 1
        ;; << local.set 0
        br_table 0 (;@2;) $hi
        ;; << i32.const 0
        ;; << local.set 0
      end
      ;; << local.get 0
      ;; << if ;; label = @2
        ;; << i32.const 34
        ;; << drop
      ;; << end
      ;; << i32.const 12
      ;; << drop
    end
    ;; << local.get 0
    ;; << if ;; label = @1
      ;; << i32.const 34
      ;; << drop
    ;; << end
    ;; << i32.const 12
    ;; << drop
  )
)
