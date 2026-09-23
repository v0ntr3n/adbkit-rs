//! Keeps track of the services of a connection.
//!
//! Ported from `src/adb/tcpusb/servicemap.ts`.

use crate::tcpusb::service::Service;

/// Maps local ids to the services handling them.
///
/// Ported from the `ServiceMap` class of `src/adb/tcpusb/servicemap.ts`.
#[derive(Debug, Default)]
pub struct ServiceMap {
    services: std::collections::HashMap<u32, Service>,
    count: usize,
}

impl ServiceMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        ServiceMap {
            services: std::collections::HashMap::new(),
            count: 0,
        }
    }

    /// Ends every service and empties the map.
    ///
    /// Ported from `ServiceMap.end()`.
    pub async fn end(&mut self) {
        for service in self.services.values_mut() {
            service.end().await;
        }
        self.services.clear();
        self.count = 0;
    }

    /// Inserts a service.
    ///
    /// Ported from `ServiceMap.insert()`.
    pub fn insert(&mut self, remote_id: u32, service: Service) -> Result<&Service, Error> {
        if self.services.contains_key(&remote_id) {
            return Err(Error::Device(format!(
                "Remote ID {} is already being used",
                remote_id
            )));
        }
        self.count += 1;
        self.services.insert(remote_id, service);
        Ok(self.services.get(&remote_id).expect("just inserted"))
    }

    /// Gets a service by its local id.
    ///
    /// Ported from `ServiceMap.get()`.
    pub fn get(&self, remote_id: u32) -> Option<&Service> {
        self.services.get(&remote_id)
    }

    /// Gets a service by its local id, mutably.
    pub fn get_mut(&mut self, remote_id: u32) -> Option<&mut Service> {
        self.services.get_mut(&remote_id)
    }

    /// Removes a service.
    ///
    /// Ported from `ServiceMap.remove()`.
    pub fn remove(&mut self, remote_id: u32) -> Option<Service> {
        let service = self.services.remove(&remote_id);
        if service.is_some() {
            self.count -= 1;
        }
        service
    }

    /// The number of services being handled.
    pub fn count(&self) -> usize {
        self.count
    }

    /// The ids of every service.
    pub fn ids(&self) -> Vec<u32> {
        self.services.keys().copied().collect()
    }
}

use crate::error::Error;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_defaults() {
        let map = ServiceMap::new();
        assert_eq!(map.count(), 0);
        assert!(map.ids().is_empty());
    }
}
