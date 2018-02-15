# Serco
### Service-Consumer framework for Rust

Inspired by serde (You couldn't tell from the name, right?) and WCF.

```


                     +-------------------------------+
                     | #[service_contract] MyService |
                     +-------------------------------+
                     | fn do_things(&self, String )  |
                     | fn get_stuff(&self) -> String |
                     +-------------------------------+
                                     A
                                     |
                  +-------------------------------------+
                  |                                     |
 +-----------------------------------+      +-------------------------+
 | #[service(MyService)] RealService |      | ServiceProxy<MyService> |
 +-----------------------------------+      +-------------------------+
 | impl MyService for Self           |      | impl MyService for Self |
 +-----------------------------------+      +-------------------------+
                  A                                     ^
                  |                                     '
      +------------------------+                        '
      | ServiceHost<MyService> |                        '
      +------------------------+            <<creates>> '
          A                                             '
          |  +--------------+                           '
          +--| HttpEndpoint |                           '
          |  +--------------+                           '
          |  +-------------+   <<connect>>   +----------------------+
          +--| TcpEndpoint | < - - - - - - - | TcpClient<MyService> |
          |  +-------------+                 +----------------------+
          |  +-------------------+
          +--| NamedPipeEndpoint |
             +-------------------+
```
