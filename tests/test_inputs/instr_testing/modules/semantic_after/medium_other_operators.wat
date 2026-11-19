(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    block $hi
      block ;; label = @2
        br_on_cast 0 (ref 0) (ref 0)
        br_on_cast_fail 0 (ref 0) (ref 0)
        br_on_null 0
        br_on_non_null 0
      end
      ;; << i32.const 12
      ;; << drop
    end
    ;; << i32.const 12
    ;; << drop
  )
  (memory (;0;) 1)
)
