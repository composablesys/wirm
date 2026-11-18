(module
  (type (;0;) (func (param i32)))
  (func (;0;) (type 0) (param i32)
    ;; << (local i32)
    block ;; label = @1
      block ;; label = @2
        local.get 0
        ;; << i32.const 1
        ;; << local.set 1
        br_table 0 (;@2;) 1 (;@1;)
        ;; << i32.const 0
        ;; << local.set 1
      end
      ;; << local.get 1
      ;; << if ;; label = @2
      ;; <<   i32.const 1234
      ;; <<   drop
      ;; << end
    end
    ;; << local.get 1
    ;; << if ;; label = @1
    ;; <<   i32.const 1234
    ;; <<   drop
    ;; << end
    block ;; label = @1
      ;; << i32.const 1
      ;; << local.set 1
      br 0 (;@1;)
      ;; << i32.const 0
      ;; << local.set 1
    end
    ;; << local.get 1
    ;; << if ;; label = @1
    ;; <<   i32.const 1234
    ;; <<   drop
    ;; << end
  )
)
