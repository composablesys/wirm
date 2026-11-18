(module
  (type (;0;) (func (param i32 i32)))
  (func (;0;) (type 0) (param i32 i32)
    ;; << (local i32)
    loop ;; label = @1
      local.get 0
      ;; << local.tee 2
      ;; << if ;; label = @2
        ;; << i32.const 1234
        ;; << drop
      ;; << end
      ;; << local.get 2
      br_if 0 (;@1;)
      local.get 1
      ;; << local.tee 2
      ;; << if ;; label = @2
        ;; << i32.const 1234
        ;; << drop
      ;; << end
      ;; << local.get 2
      br_if 0 (;@1;)
    end
  )
)
