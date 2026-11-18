(module
  (type (;0;) (func))
  (memory (;0;) 1)
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
        i32.const 1
        if ;; label = @3
          ;; << i32.const 45
          ;; << drop
          br 0 (;@3;)
        else
          ;; << i32.const 45
          ;; << drop
          br 1 (;@2;)
        end
        ;; << i32.const 23
        ;; << drop
        ;; << i32.const 34
        ;; << drop
      end
      ;; << local.get 0
      ;; << if ;; label = @2
        ;; << i32.const 56
        ;; << drop
      ;; << end
      ;; << i32.const 12
      ;; << drop
    end
    ;; << local.get 0
    ;; << if ;; label = @1
      ;; << i32.const 56
      ;; << drop
    ;; << end
    ;; << i32.const 12
    ;; << drop
  )
)
