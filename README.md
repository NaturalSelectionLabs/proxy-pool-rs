# Proxy Pool Tools

Use local ipv4/ipv6 proxy pool to crawl web pages.

Inspired by [http-proxy-ipv6-pool](https://zu1k.com/posts/tutorials/http-proxy-ipv6-pool/)

## Get Started

### Install

TODO

### Prepare

Get IPv6 subnet from your VPS provider, and add it to your server.

```bash
$ ip a
2: ens4: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1460 qdisc mq state UP group default qlen 1000
    link/ether 42:01:0a:02:00:02 brd ff:ff:ff:ff:ff:ff
    altname enp0s4
    inet 10.2.0.2/32 metric 100 scope global dynamic ens4
       valid_lft 2455sec preferred_lft 2455sec
    inet6 2600:1900:4000:39cb::/128 scope global dynamic
       valid_lft 3214sec preferred_lft 3214sec
    inet6 fe80::4001:aff:fe02:2/64 scope link
       valid_lft forever preferred_lft forever
```

Add route via interface `ens4`:

```bash
sudo ip route add local 2600:1900:4000:39cb::/64 dev ens4
```

Set ip nonlocal bind:

```bash
sudo sysctl -w net.ipv6.ip_nonlocal_bind=1
```

For IPv6 NDP, install and start `ndppd`:

```bash
sudo apt update && sudo apt install -y ndppd
```

then edit `/etc/ndppd.conf`:

```bash
route-ttl 30000

proxy <INTERFACE-NAME> {
    router no
    timeout 500
    ttl 30000

    rule <IP6_SUBNET> {
        static
    }
}
```

and start `ndppd`:

```bash
sudo systemctl start ndppd
```

Now you can use the IPv6 subnet to create proxy pool.

```bash
$ curl --interface 2600:1900:4000:39cb::1 ifv6.ip.sb
2600:1900:4000:39cb::1
$ curl --interface 2600:1900:4000:39cb::2 ifv6.ip.sb
2600:1900:4000:39cb::2
```
