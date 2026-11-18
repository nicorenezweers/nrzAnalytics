//! Tracking-Skript (`/js/script.js` und Varianten), kompatibel mit dem Skript von Plausible.
//!
//! Das Skript wird aus Bausteinen zusammengesetzt: ein Kern (Senden, Seitenaufrufe, Engagement)
//! und je Erweiterung ein eigener Teil. Kleine, erweiterungsabhaengige Stellen im Kern sind mit
//! Markern (`/*@name*/`) gekennzeichnet, die beim Zusammenbau ersetzt oder entfernt werden.
//! Das JavaScript bleibt bewusst ES5-nah (kein `let`/`const`, keine Pfeilfunktionen).

/// Gewaehlte Erweiterungen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Features {
    hash: bool,
    outbound: bool,
    downloads: bool,
    tagged: bool,
    revenue: bool,
    pageview_props: bool,
    manual: bool,
    local: bool,
    exclusions: bool,
}

/// Name zerlegen: `script|plausible|nrz` gefolgt von Erweiterungen (`.hash.manual` …).
fn parse_name(name: &str) -> Option<Features> {
    let mut parts = name.split('.');
    if !matches!(parts.next()?, "script" | "plausible" | "nrz") {
        return None;
    }
    let mut f = Features::default();
    for part in parts {
        match part {
            "hash" => f.hash = true,
            "outbound-links" => f.outbound = true,
            "file-downloads" => f.downloads = true,
            "tagged-events" => f.tagged = true,
            "revenue" => f.revenue = true,
            "pageview-props" => f.pageview_props = true,
            "manual" => f.manual = true,
            "local" => f.local = true,
            "exclusions" => f.exclusions = true,
            // `compat` (alte Browser): Fallback auf `#plausible` ist immer enthalten.
            "compat" => {}
            _ => return None,
        }
    }
    Some(f)
}

/// Skript fuer `/js/<name>.js`, z. B. `script`, `script.hash.outbound-links`, `plausible.manual`.
/// `None` fuer unbekannte Namen oder Erweiterungen.
pub fn script_for(name: &str) -> Option<String> {
    parse_name(name).map(|f| build(&f))
}

const HEADER: &str = "/* nrzAnalytics – Tracking-Skript (kompatibel mit Plausible) */\n";

/// Kern: Konfiguration, Senden, Engagement-Messung.
const CORE: &str = r#"(function(){'use strict';
var w=window,d=document,l=w.location,n=w.navigator||{};
var s=d.currentScript||d.getElementById('plausible')||d.querySelector('script[data-domain]')||{getAttribute:function(){return null}};
var api=s.getAttribute('data-api');
if(!api){try{api=new URL(s.src).origin+'/api/event'}catch(e){api='/api/event'}}
var domain=s.getAttribute('data-domain')||l.hostname.replace(/^www\./,'');
function warn(m){if(w.console&&console.warn)console.warn('nrzAnalytics: '+m)}
function send(p,cb){
var body=JSON.stringify(p);
function done(st){if(cb)cb({status:st})}
if(w.fetch){try{w.fetch(api,{method:'POST',headers:{'Content-Type':'text/plain'},keepalive:true,body:body}).then(function(r){done(r.status)},function(){done(0)});return}catch(e){}}
var x=new XMLHttpRequest();
x.open('POST',api,true);
x.setRequestHeader('Content-Type','text/plain');
x.onreadystatechange=function(){if(x.readyState===4)done(x.status)};
x.send(body)
}
var eu=null,ep=null,et=0,es=null,emax=0,esent=-1;
function now(){return new Date().getTime()}
function vis(){return d.visibilityState==='visible'}
function act(){return vis()&&(!d.hasFocus||d.hasFocus())}
function tStart(){if(es===null&&act())es=now()}
function tStop(){if(es!==null){et+=now()-es;es=null}}
function tNow(){return et+(es!==null?now()-es:0)}
function docH(){var b=d.body||{},e=d.documentElement||{};return Math.max(b.scrollHeight||0,b.offsetHeight||0,b.clientHeight||0,e.scrollHeight||0,e.offsetHeight||0,e.clientHeight||0)}
function onScroll(){var e=d.documentElement||{},y=w.pageYOffset||e.scrollTop||(d.body&&d.body.scrollTop)||0,b=y+(w.innerHeight||e.clientHeight||0);if(b>emax)emax=b}
function depth(){var h=docH();if(h<=0)return 0;var v=Math.round(emax/h*100);return v>100?100:(v<0?0:v)}
function engage(){
if(!eu)return;
onScroll();
var t=tNow(),sd=depth();
if(t>=3000||sd>esent){
esent=sd;et=0;if(es!==null)es=now();
var p={n:'engagement',u:eu,d:domain,sd:sd,e:t};
if(ep)p.p=ep;
/*@hash*/
send(p)
}
}
function ignore(name,why,o){if(why)warn('Ereignis ignoriert: '+why);if(name==='pageview')eu=null;if(o&&o.callback)o.callback()}
"#;

/// Erweiterung `exclusions`: `data-include`/`data-exclude` mit `*` und `**`.
const EXCLUSIONS: &str = r#"var inc=s.getAttribute('data-include'),exc=s.getAttribute('data-exclude');
function pm(pat){var path=l.pathname/*@hashpath*/;var re=pat.trim().replace(/[.+?^${}()|[\]\\]/g,'\\$&').replace(/\*\*/g,'\u0001').replace(/\*/g,'[^\\s/]*').replace(/\u0001/g,'.*');try{return new RegExp('^'+re+'/?$').test(path)}catch(e){return false}}
function anyPath(list){if(!list)return false;var a=list.split(',');for(var i=0;i<a.length;i++)if(a[i].trim()&&pm(a[i]))return true;return false}
"#;

/// Ereignis ausloesen (`window.plausible`).
const TRIGGER: &str = r#"function trigger(name,o){
o=o||{};
if(name==='pageview')engage();
/*@local*/
if(w._phantom||w.__nightmare||n.webdriver||w.Cypress)return ignore(name,null,o);
try{if(w.localStorage&&w.localStorage.plausible_ignore==='true')return ignore(name,'localStorage-Flag plausible_ignore',o)}catch(e){}
/*@excl*/
var p={n:name,u:o.u||/*@url*/,d:domain,r:d.referrer||null};
if(o.props)p.p=o.props;
/*@pvprops*/
/*@revenue*/
if(o.interactive===false)p.i=false;
/*@hash*/
p.w=w.innerWidth;
if(name==='pageview'){eu=p.u;ep=p.p||null;et=0;es=null;tStart();emax=0;onScroll();esent=-1}
send(p,o.callback)
}
var q=[].concat((w.plausible&&w.plausible.q)||[],(w.nrz&&w.nrz.q)||[]);
w.plausible=trigger;w.nrz=trigger;
for(var i=0;i<q.length;i++)trigger.apply(this,q[i]);
d.addEventListener('visibilitychange',function(){if(vis())tStart();else{tStop();engage()}});
w.addEventListener('focus',tStart);
w.addEventListener('blur',tStop);
w.addEventListener('pagehide',function(){tStop();engage()});
w.addEventListener('scroll',onScroll,{passive:true});
w.addEventListener('load',onScroll);
"#;

/// Automatische Seitenaufrufe (ausser bei `manual`).
const AUTO: &str = r#"var last=null;
function page(spa){var u=/*@url*/;if(spa&&last===u)return;last=u;trigger('pageview')}
"#;

/// SPA-Navigation ueber die History-API.
const SPA_HISTORY: &str = r#"var his=w.history;
if(his&&his.pushState){var ps=his.pushState;his.pushState=function(){var r=ps.apply(this,arguments);page(true);return r};w.addEventListener('popstate',function(){page(true)})}
"#;

/// SPA-Navigation ueber den Hash (`hash`).
const SPA_HASH: &str = r#"w.addEventListener('hashchange',function(){page(true)});
"#;

/// Start, bei Prerendering erst bei Sichtbarkeit.
const AUTO_START: &str = r#"if(d.visibilityState==='prerender'){d.addEventListener('visibilitychange',function f(){if(d.visibilityState==='visible'&&last===null){d.removeEventListener('visibilitychange',f);page(false)}})}else page(false);
"#;

/// Gemeinsame Hilfen fuer Link-Klicks (Outbound, Downloads, getaggte Links).
const LINKS: &str = r#"function isA(e){return !!(e&&e.tagName&&e.tagName.toLowerCase()==='a')}
function follow(ev,a){if(ev.defaultPrevented)return false;var t=!a.target||/^_(self|parent|top)$/i.test(a.target);var reg=!(ev.ctrlKey||ev.metaKey||ev.shiftKey)&&ev.type==='click';return t&&reg&&!(a.hasAttribute&&a.hasAttribute('download'))}
function linkEvent(ev,a,ea){
var done=false;function go(){if(!done){done=true;w.location=a.href}}
var x={props:ea.props};
/*@revenue_attrs*/
if(follow(ev,a)){x.callback=go;trigger(ea.name,x);setTimeout(go,5000);ev.preventDefault()}else trigger(ea.name,x)
}
"#;

/// Erweiterung `file-downloads`: Dateitypen (per `file-types`/`add-file-types` anpassbar).
const DOWNLOADS: &str = r#"var ft=['pdf','xlsx','docx','txt','rtf','csv','exe','key','pps','ppt','pptx','7z','pkg','rar','gz','zip','avi','mov','mp4','mpeg','wmv','midi','mp3','wav','wma','dmg','iso','msi'];
var fa=s.getAttribute('file-types'),fb=s.getAttribute('add-file-types');
if(fa)ft=fa.split(',');if(fb)ft=ft.concat(fb.split(','));
function isDl(u){var e=u.split('.').pop().toLowerCase();for(var i=0;i<ft.length;i++)if(ft[i].trim().toLowerCase()===e)return true;return false}
"#;

/// Klick-Erkennung fuer Outbound-Links und Downloads (auch Mittelklick).
const LINK_CLICKS: &str = r#"function getA(e){while(e&&(typeof e.tagName==='undefined'||!isA(e)||!e.href))e=e.parentNode;return e}
function onLink(ev){
if(ev.type==='auxclick'&&ev.button!==1)return;
var a=getA(ev.target);if(!a)return;
var bare=a.href.split('#')[0].split('?')[0];
/*@tagged_skip*/
/*@outbound*/
/*@downloads*/
}
d.addEventListener('click',onLink);d.addEventListener('auxclick',onLink);
"#;

/// Erweiterung `tagged-events`: Klassen `plausible-event-name=…` an Elementen, Links, Formularen.
const TAGGED: &str = r#"function isTagged(e){var c=e&&e.classList;if(c)for(var i=0;i<c.length;i++)if(/plausible-event-name(=|--)(.+)/.test(c.item(i)))return true;return false}
function tagged(e,lv){if(!e||lv>3)return false;if(isTagged(e))return true;return tagged(e.parentNode,lv+1)}
function attrs(e){var t=isTagged(e)?e:(e&&e.parentNode);var r={name:null,props:{},revenue:{}};var c=t&&t.classList;if(!c)return r;for(var i=0;i<c.length;i++){var cn=c.item(i),m=cn.match(/plausible-event-(.+)(=|--)(.+)/);if(m){var k=m[1],v=m[3].replace(/\+/g,' ');if(k.toLowerCase()==='name')r.name=v;else r.props[k]=v}var rm=cn.match(/plausible-revenue-(.+)(=|--)(.+)/);if(rm)r.revenue[rm[1]]=rm[3]}return r}
function isForm(e){return !!(e&&e.tagName&&e.tagName.toLowerCase()==='form')}
function onSubmit(ev){var f=ev.target,ea=attrs(f);if(!ea.name)return;ev.preventDefault();var done=false;function go(){if(!done){done=true;f.submit()}}setTimeout(go,5000);var x={props:ea.props,callback:go};
/*@revenue_attrs*/
trigger(ea.name,x)}
function onTagClick(ev){if(ev.type==='auxclick'&&ev.button!==1)return;var e=ev.target,a=null,t=null;for(var i=0;i<=3;i++){if(!e)break;if(isForm(e))return;if(isA(e))a=e;if(isTagged(e))t=e;e=e.parentNode}if(!t)return;var ea=attrs(t);if(!ea.name)return;if(a){ea.props.url=a.href;linkEvent(ev,a,ea)}else{var x={props:ea.props};
/*@revenue_attrs*/
trigger(ea.name,x)}}
d.addEventListener('submit',onSubmit);d.addEventListener('click',onTagClick);d.addEventListener('auxclick',onTagClick);
"#;

const FOOTER: &str = "})();\n";

/// Baustein `local` fehlt: localhost und Dateien nicht erfassen.
const LOCAL_CHECK: &str = r#"if(/^localhost$|^127\.|^\[::1\]$/.test(l.hostname)||l.protocol==='file:')return ignore(name,'localhost',o);"#;
const EXCL_CHECK: &str = r#"if((inc&&!anyPath(inc))||anyPath(exc))return ignore(name,'Ausschlussregel',o);"#;
const PAGEVIEW_PROPS: &str = r#"var at=s.attributes,pp={},has=false,k,j;if(p.p)for(k in p.p){pp[k]=p.p[k];has=true}if(at)for(j=0;j<at.length;j++){k=at[j].name;if(k.substring(0,6)==='event-'){k=k.substring(6);if(!(k in pp)){pp[k]=at[j].value;has=true}}}if(has)p.p=pp;"#;
const REVENUE: &str = r#"if(o.revenue)p.$=o.revenue;"#;
const REVENUE_ATTRS: &str = r#"if(ea.revenue&&ea.revenue.amount)x.revenue=ea.revenue;"#;
const TAGGED_SKIP: &str = r#"if(tagged(a,0))return;"#;
const OUTBOUND: &str = r#"if(a.host&&a.host!==l.host&&/^https?:$/.test(a.protocol))return linkEvent(ev,a,{name:'Outbound Link: Click',props:{url:a.href}});"#;
const DOWNLOAD_CHECK: &str =
    r#"if(isDl(bare))return linkEvent(ev,a,{name:'File Download',props:{url:bare}});"#;

/// Skript fuer die gewaehlten Erweiterungen zusammensetzen.
fn build(f: &Features) -> String {
    let links = f.outbound || f.downloads || f.tagged;
    let mut parts: Vec<&str> = vec![HEADER, CORE];
    if f.exclusions {
        parts.push(EXCLUSIONS);
    }
    parts.push(TRIGGER);
    if !f.manual {
        parts.push(AUTO);
        parts.push(if f.hash { SPA_HASH } else { SPA_HISTORY });
        parts.push(AUTO_START);
    }
    if links {
        parts.push(LINKS);
    }
    if f.downloads {
        parts.push(DOWNLOADS);
    }
    if f.outbound || f.downloads {
        parts.push(LINK_CLICKS);
    }
    if f.tagged {
        parts.push(TAGGED);
    }
    parts.push(FOOTER);
    let mut js = parts.concat();

    let pick = |on: bool, s: &'static str| if on { s } else { "" };
    let markers: [(&str, &str); 11] = [
        ("/*@local*/", pick(!f.local, LOCAL_CHECK)),
        ("/*@excl*/", pick(f.exclusions, EXCL_CHECK)),
        ("/*@url*/", if f.hash { "l.href" } else { "l.href.split('#')[0]" }),
        ("/*@hash*/", pick(f.hash, "p.h=1;")),
        ("/*@hashpath*/", pick(f.hash, "+l.hash")),
        ("/*@pvprops*/", pick(f.pageview_props, PAGEVIEW_PROPS)),
        ("/*@revenue*/", pick(f.revenue, REVENUE)),
        ("/*@revenue_attrs*/", pick(f.revenue, REVENUE_ATTRS)),
        ("/*@tagged_skip*/", pick(f.tagged, TAGGED_SKIP)),
        ("/*@outbound*/", pick(f.outbound, OUTBOUND)),
        ("/*@downloads*/", pick(f.downloads, DOWNLOAD_CHECK)),
    ];
    for (marker, snippet) in markers {
        if js.contains(marker) {
            js = js.replace(marker, snippet);
        }
    }
    // Durch leere Marker entstandene Leerzeilen entfernen.
    while js.contains("\n\n") {
        js = js.replace("\n\n", "\n");
    }
    js
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Bekannte Erweiterungen (Namensteile nach dem Punkt).
    const EXTENSIONS: &[&str] = &[
        "hash",
        "outbound-links",
        "file-downloads",
        "tagged-events",
        "revenue",
        "pageview-props",
        "manual",
        "local",
        "exclusions",
        "compat",
    ];

    /// Alle Namen: jede Kombination der Erweiterungen (ohne `compat`) mit allen drei Praefixen
    /// ist zu viel – hier: alle Kombinationen mit `script`.
    fn all_variants() -> Vec<String> {
        let exts: Vec<&str> = EXTENSIONS.iter().copied().filter(|e| *e != "compat").collect();
        (0u32..(1 << exts.len()))
            .map(|mask| {
                let mut name = String::from("script");
                for (i, e) in exts.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        name.push('.');
                        name.push_str(e);
                    }
                }
                name
            })
            .collect()
    }

    #[test]
    fn namen_und_aliase() {
        for base in ["script", "plausible", "nrz"] {
            assert!(script_for(base).is_some(), "{base}");
            for e in EXTENSIONS {
                assert!(script_for(&format!("{base}.{e}")).is_some(), "{base}.{e}");
            }
        }
        assert_eq!(script_for("plausible.hash"), script_for("script.hash"));
        assert_eq!(script_for("nrz.manual"), script_for("script.manual"));
        assert_eq!(script_for("script.compat"), script_for("script"));
        // Reihenfolge egal, doppelte Erweiterungen unschaedlich.
        assert_eq!(
            script_for("script.hash.outbound-links.tagged-events"),
            script_for("script.tagged-events.hash.outbound-links")
        );
        assert_eq!(script_for("script.hash.hash"), script_for("script.hash"));
    }

    #[test]
    fn unbekannte_namen() {
        for name in [
            "",
            "foo",
            "Script",
            "script.",
            "script..hash",
            "script.unbekannt",
            "script.hash.unbekannt",
            "scripts",
            ".hash",
            "script.HASH",
            "script.js",
        ] {
            assert!(script_for(name).is_none(), "{name}");
        }
    }

    #[test]
    fn inhalte_grundskript() {
        let js = script_for("script").unwrap();
        assert!(js.starts_with("/* nrzAnalytics – Tracking-Skript (kompatibel mit Plausible) */"));
        for needle in [
            "data-api",
            "/api/event",
            "data-domain",
            "keepalive:true",
            "'Content-Type':'text/plain'",
            "XMLHttpRequest",
            "w.plausible=trigger",
            "w.nrz=trigger",
            "w.plausible.q",
            "pushState",
            "popstate",
            "prerender",
            "localhost",
            "_phantom",
            "__nightmare",
            "webdriver",
            "Cypress",
            "plausible_ignore",
            "n:'engagement'",
            "sd:sd",
            "e:t",
            "visibilitychange",
            "pagehide",
            "innerWidth",
            "d.referrer||null",
            "p.i=false",
            "o.callback",
            "split('#')[0]",
        ] {
            assert!(js.contains(needle), "fehlt: {needle}");
        }
        for absent in [
            "hashchange",
            "p.h=1",
            "Outbound Link: Click",
            "File Download",
            "plausible-event-name",
            "p.$",
            "event-",
            "data-include",
            "/*@",
        ] {
            assert!(!js.contains(absent), "unerwartet: {absent}");
        }
    }

    #[test]
    fn inhalte_erweiterungen() {
        let has = |name: &str, needle: &str| script_for(name).unwrap().contains(needle);
        assert!(has("script.hash", "hashchange"));
        assert!(has("script.hash", "p.h=1"));
        assert!(!has("script.hash", "pushState"));
        assert!(!has("script.hash", "split('#')[0]"));
        assert!(has("script.outbound-links", "Outbound Link: Click"));
        assert!(has("script.outbound-links", "auxclick"));
        assert!(has("script.outbound-links", "setTimeout(go,5000)"));
        assert!(!has("script.outbound-links", "File Download"));
        assert!(has("script.file-downloads", "File Download"));
        assert!(has("script.file-downloads", "add-file-types"));
        assert!(has("script.file-downloads", "'msi'"));
        assert!(has("script.tagged-events", "plausible-event-name"));
        assert!(has("script.tagged-events", "'submit'"));
        assert!(
            !has("script.tagged-events", "plausible-revenue") || has("script.tagged-events", "revenue:{}")
        );
        assert!(has("script.revenue", "p.$=o.revenue"));
        assert!(has("script.tagged-events.revenue", "x.revenue=ea.revenue"));
        assert!(has("script.pageview-props", "'event-'"));
        assert!(!has("script.manual", "pushState"));
        assert!(!has("script.manual", "page(false)"));
        assert!(has("script.manual", "w.plausible=trigger"));
        assert!(!has("script.local", "localhost"));
        assert!(has("script.exclusions", "data-include"));
        assert!(has("script.exclusions", "data-exclude"));
        assert!(has("script.exclusions.hash", "+l.hash"));
        for name in all_variants() {
            let js = script_for(&name).unwrap();
            assert!(!js.contains("/*@"), "Marker übrig in {name}");
            assert!(!js.contains("\n\n"), "Leerzeile in {name}");
        }
    }

    #[test]
    fn grundskript_ist_klein() {
        let js = script_for("script").unwrap();
        assert!(js.len() < 6000, "{} Bytes", js.len());
        let alles = script_for(&all_variants().pop().unwrap()).unwrap();
        assert!(alles.len() > js.len());
        assert!(alles.len() < 12000, "{} Bytes", alles.len());
    }

    /// JavaScript-Laufzeit suchen (`node`, sonst `deno`).
    fn js_runtime() -> Option<(String, Vec<&'static str>)> {
        let ok = |bin: &str| {
            Command::new(bin)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        for bin in ["node", "/opt/homebrew/bin/node", "/usr/local/bin/node"] {
            if ok(bin) {
                return Some((bin.to_string(), vec![]));
            }
        }
        for bin in ["deno", "/opt/homebrew/bin/deno"] {
            if ok(bin) {
                return Some((bin.to_string(), vec!["run", "--quiet"]));
            }
        }
        None
    }

    /// Fuehrt `code` mit node/deno aus; `None`, wenn keine Laufzeit vorhanden ist.
    fn run_js(code: &str, tag: &str) -> Option<(bool, String)> {
        let (bin, args) = js_runtime()?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "nrzanalytics-tracker-{tag}-{}-{nanos}.js",
            std::process::id()
        ));
        std::fs::write(&path, code).ok()?;
        let out = Command::new(&bin).args(&args).arg(&path).output();
        let _ = std::fs::remove_file(&path);
        let out = out.ok()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        Some((out.status.success(), text))
    }

    #[test]
    fn syntax_aller_varianten() {
        let variants: Vec<String> = all_variants()
            .iter()
            .map(|n| script_for(n).unwrap_or_default())
            .collect();
        let names = serde_json::to_string(&all_variants()).unwrap_or_default();
        let srcs = serde_json::to_string(&variants).unwrap_or_default();
        // `new Function` parst den Code nur (fuehrt ihn nicht aus).
        let code = format!(
            "var names={names};var srcs={srcs};var bad=0;\
             for(var i=0;i<srcs.length;i++){{try{{new Function(srcs[i])}}catch(e){{bad++;console.log(names[i]+': '+e.message)}}}}\
             if(bad)throw new Error(bad+' Varianten fehlerhaft');console.log('ok '+srcs.length);"
        );
        match run_js(&code, "syntax") {
            None => eprintln!("node/deno nicht gefunden – Syntaxprüfung übersprungen"),
            Some((ok, out)) => assert!(ok, "{out}"),
        }
    }

    /// Fuehrt das Skript mit einem minimalen DOM aus und prueft die gesendeten Ereignisse.
    #[test]
    fn laufzeit_mit_attrappen_dom() {
        let script = |n: &str| serde_json::to_string(&script_for(n).unwrap_or_default()).unwrap_or_default();
        let code = format!(
            r#"
var sent=[];var listeners={{}};
function on(t,f){{(listeners[t]=listeners[t]||[]).push(f)}}
function fire(t,ev){{(listeners[t]||[]).forEach(function(f){{f(ev||{{}})}})}}
function setup(host){{
  sent=[];listeners={{}};
  var attrs={{'data-domain':'example.com','event-author':'Nico'}};
  var g=globalThis;
  g.window=g;
  g.document={{currentScript:{{src:'https://stats.example.org/js/script.js',getAttribute:function(k){{return k in attrs?attrs[k]:null}},
    attributes:Object.keys(attrs).map(function(k){{return {{name:k,value:attrs[k]}}}})}},
    referrer:'https://news.ycombinator.com/',visibilityState:'visible',hasFocus:function(){{return true}},
    addEventListener:on,removeEventListener:function(){{}},body:{{scrollHeight:2000}},documentElement:{{clientHeight:1000,scrollTop:0}},
    getElementById:function(){{return null}},querySelector:function(){{return null}}}};
  g.location={{href:'https://'+host+'/a?x=1#teil',hostname:host,host:host,pathname:'/a',hash:'#teil',protocol:'https:'}};
  try{{Object.defineProperty(g,'navigator',{{value:{{webdriver:false}},configurable:true,writable:true}})}}catch(e){{}}
  g.innerWidth=1280;g.innerHeight=1000;g.pageYOffset=0;
  g.history={{pushState:function(a,b,u){{g.location.href='https://'+host+u;g.location.pathname=u}}}};
  g.addEventListener=on;
  try{{Object.defineProperty(g,'localStorage',{{value:{{}},configurable:true,writable:true}})}}catch(e){{}}
  g.fetch=function(u,o){{sent.push({{url:u,body:JSON.parse(o.body),opts:o}});return {{then:function(ok){{ok({{status:202}})}}}}}};
  delete g.plausible;delete g.nrz;
}}
function assert(c,m){{if(!c)throw new Error(m+' '+JSON.stringify(sent))}}

// 1) Standard: Seitenaufruf beim Laden
setup('example.com');
new Function({base})();
assert(sent.length===1,'ein Seitenaufruf');
var b=sent[0].body;
assert(sent[0].url==='https://stats.example.org/api/event','Endpunkt');
assert(sent[0].opts.keepalive===true,'keepalive');
assert(b.n==='pageview'&&b.d==='example.com'&&b.u==='https://example.com/a?x=1'&&b.w===1280,'Payload');
assert(b.r==='https://news.ycombinator.com/','Referrer');
assert(!('h' in b),'kein h');
// SPA-Navigation: Engagement fuer alte Seite, dann neuer Seitenaufruf
history.pushState({{}},'','/b');
assert(sent.length===3,'Engagement + Seitenaufruf');
assert(sent[1].body.n==='engagement'&&sent[1].body.u==='https://example.com/a?x=1','Engagement');
assert(typeof sent[1].body.sd==='number'&&sent[1].body.sd>=0&&sent[1].body.sd<=100,'Scrolltiefe');
assert(sent[2].body.n==='pageview'&&sent[2].body.u==='https://example.com/b','zweiter Seitenaufruf');
// gleiche URL nochmal: kein doppelter Seitenaufruf
history.pushState({{}},'','/b');
assert(sent.length===3,'kein Duplikat');
// eigenes Ereignis mit Optionen
var cbStatus=null;
plausible('Signup',{{props:{{plan:'pro'}},interactive:false,callback:function(r){{cbStatus=r&&r.status}}}});
var e=sent[sent.length-1].body;
assert(e.n==='Signup'&&e.p.plan==='pro'&&e.i===false,'Ereignis');
assert(cbStatus===202,'callback');
// Seite verstecken -> Engagement (Scrolltiefe gestiegen)
window.pageYOffset=1000;fire('scroll');
document.visibilityState='hidden';fire('visibilitychange');
var g2=sent[sent.length-1].body;
assert(g2.n==='engagement'&&g2.sd===100,'Engagement beim Verstecken');
var vorher=sent.length;fire('pagehide');
assert(sent.length===vorher,'kein leeres Engagement');

// 2) localhost wird ignoriert, callback trotzdem
setup('localhost');
new Function({base})();
assert(sent.length===0,'localhost ignoriert');
var called=false;plausible('X',{{callback:function(){{called=true}}}});
assert(called&&sent.length===0,'callback bei ignoriert');
// ... ausser mit local
setup('localhost');
new Function({local})();
assert(sent.length===1,'local erfasst');

// 3) manual + Warteschlange + pageview-props + revenue
setup('example.com');
window.plausible=function(){{(window.plausible.q=window.plausible.q||[]).push(arguments)}};
plausible('pageview',{{u:'https://example.com/eigene'}});
plausible('Kauf',{{revenue:{{amount:9.5,currency:'EUR'}}}});
new Function({manual})();
assert(sent.length===2,'Warteschlange abgearbeitet');
assert(sent[0].body.u==='https://example.com/eigene'&&sent[0].body.p.author==='Nico','u + pageview-props');
assert(sent[1].body.$.currency==='EUR','Revenue');

// 4) hash
setup('example.com');
new Function({hash})();
assert(sent[0].body.h===1&&sent[0].body.u==='https://example.com/a?x=1#teil','hash');
location.href='https://example.com/a?x=1#neu';fire('hashchange');
assert(sent[sent.length-1].body.u==='https://example.com/a?x=1#neu','hashchange');

// 5) Webdriver wird ignoriert
setup('example.com');navigator.webdriver=true;
new Function({base})();
assert(sent.length===0,'webdriver ignoriert');

// 6) exclusions
setup('example.com');document.currentScript.getAttribute=function(k){{return k==='data-exclude'?'/a, /admin/**':(k==='data-domain'?'example.com':null)}};
new Function({excl})();
assert(sent.length===0,'ausgeschlossen');
history.pushState({{}},'','/blog/x');
assert(sent.length===1&&sent[0].body.u==='https://example.com/blog/x','nicht ausgeschlossen');
console.log('ok');
"#,
            base = script("script"),
            local = script("script.local"),
            manual = script("script.manual.pageview-props.revenue"),
            hash = script("script.hash"),
            excl = script("script.exclusions"),
        );
        match run_js(&code, "laufzeit") {
            None => eprintln!("node/deno nicht gefunden – Laufzeittest übersprungen"),
            Some((ok, out)) => assert!(ok, "{out}"),
        }
    }
}
