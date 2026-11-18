(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    loop ;; label = @1
      block ;; label = @2
        loop ;; label = @3
          ;; << i32.const 1234
          ;; << drop
          br 2 (;@1;)
        end
      end
    end
  )
)
