(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    block ;; label = @1
      block ;; label = @2
        i32.const 0
        br_if 0 (;@2;)
        i32.const 0
        br_table 0 (;@2;)
        if ;; label = @3
          ;; << i32.const 34
          ;; << drop
          br 0 (;@3;)
        end
        ;; << i32.const 12
        ;; << drop
      end
    end
  )
)
