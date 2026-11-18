(module
  (type (;0;) (struct))
  (type (;1;) (func (param (ref 0))))
  (func (;0;) (type 1) (param (ref 0))
    ;; << (local (ref 0))
    local.get 0
    loop (type 1) (param (ref 0)) ;; label = @1
      ;; << local.tee 1
      ;; << ref.test (ref 0)
      ;; << if ;; label = @2
        ;; << i32.const 1234
        ;; << drop
      ;; << end
      ;; << local.get 1
      br_on_cast 0 (;@1;) (ref 0) (ref 0)
      drop
    end
  )
)
