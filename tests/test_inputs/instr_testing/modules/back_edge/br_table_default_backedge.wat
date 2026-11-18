(module
  (type (;0;) (func (param i32)))
  (func (;0;) (type 0) (param i32)
    ;; << (local i32)
    block ;; label = @1
      loop ;; label = @2
        local.get 0
        ;; << local.set 1
        ;; << local.get 1
        ;; << i32.const 1
        ;; << i32.ge_u
        ;; << if ;; label = @3
          ;; << i32.const 1234
          ;; << drop
        ;; << end
        ;; << local.get 1
        br_table 1 (;@1;) 0 (;@2;)
      end
    end
  )
)
