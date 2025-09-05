(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    ;; << (local i32 i32 i32 i32)
    block ;; label = @1
      block ;; label = @2
        i32.const 0
        if ;; label = @3
          i32.const 1
          drop
        end
        ;; << i32.const 34
        ;; << drop
        i32.const 0
        if ;; label = @3
          i32.const 1
          drop
        else
          i32.const 0
          ;; << i32.const 1
          ;; << local.set 0
          br_if 2 (;@1;)
          ;; << i32.const 0
          ;; << local.set 0
          ;; << i32.const 67
          ;; << drop
        end
        ;; << i32.const 34
        ;; << drop
        ;; << i32.const 45
        ;; << drop
        block ;; label = @3
          block ;; label = @4
            i32.const 1
            ;; << i32.const 1
            ;; << local.set 1
            br_table 3 (;@1;) 2 (;@2;) 0 (;@4;)
            ;; << i32.const 0
            ;; << local.set 1
            block ;; label = @5
              i32.const 1
              ;; << i32.const 1
              ;; << local.set 2
              br_table 4 (;@1;) 1 (;@4;)
              ;; << i32.const 0
              ;; << local.set 2
            end
            ;; << i32.const 12
            ;; << drop
          end
          ;; << local.get 1
          ;; << if ;; label = @4
          ;; <<   i32.const 78
          ;; <<   drop
          ;; << else
          ;; <<   local.get 2
          ;; <<   if ;; label = @5
          ;; <<     i32.const 78
          ;; <<     drop
          ;; <<   end
          ;; << end
          ;; << i32.const 12
          ;; << drop
          loop ;; label = @4
            i32.const 0
            ;; << i32.const 1
            ;; << local.set 3
            br 0 (;@4;)
            ;; << i32.const 0
            ;; << local.set 3
          end
          ;; << local.get 3
          ;; << if ;; label = @4
          ;; <<   i32.const 56
          ;; <<   drop
          ;; << end
          ;; << i32.const 23
          ;; << drop
        end
        ;; << i32.const 12
        ;; << drop
      end
      ;; << local.get 1
      ;; << if ;; label = @2
      ;; <<   i32.const 78
      ;; <<   drop
      ;; << end
      ;; << i32.const 12
      ;; << drop
    end
    ;; << local.get 0
    ;; << if ;; label = @1
    ;; <<   i32.const 67
    ;; <<   drop
    ;; << else
    ;; <<   local.get 1
    ;; <<   if ;; label = @2
    ;; <<     i32.const 78
    ;; <<     drop
    ;; <<   else
    ;; <<     local.get 2
    ;; <<     if ;; label = @2
    ;; <<       i32.const 78
    ;; <<       drop
    ;; <<     end
    ;; <<   end
    ;; << end
    ;; << i32.const 12
    ;; << drop
  )
  (memory (;0;) 1)
)
